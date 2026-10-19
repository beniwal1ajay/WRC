//! H.264 Video Encoder implementation using OpenH264.
//!
//! Encodes raw frames (BGRx/BGRA) from PipeWire screen capture
//! into standard H.264 Annex B NAL units (I-frames, P-frames) for network streaming.

pub mod adaptive;

use anyhow::{Context, Result};
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, RateControlMode, UsageType,
    VuiConfig,
};
use openh264::formats::YUVSource;

/// Zero-copy borrow of planar YUV420 data implementing OpenH264's YUVSource trait.
struct BorrowedYuv<'a> {
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
    width: usize,
    height: usize,
}

impl<'a> YUVSource for BorrowedYuv<'a> {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        self.y
    }

    fn u(&self) -> &[u8] {
        self.u
    }

    fn v(&self) -> &[u8] {
        self.v
    }
}
use tracing::{debug, info};

/// An encoded video packet ready for transmission.
#[derive(Debug, Clone)]
pub struct EncodedPacket {
    /// Annex B H.264 NAL units (including start codes 0x00000001).
    pub data: Vec<u8>,
    /// Whether this frame is an IDR/I keyframe.
    pub is_keyframe: bool,
    /// Presentation timestamp in nanoseconds.
    pub timestamp_ns: u64,
}

/// Configuration options for the video encoder.
#[derive(Debug, Clone)]
pub struct EncoderSettings {
    pub width: usize,
    pub height: usize,
    pub bitrate_bps: u32,
    pub max_fps: f32,
}

impl Default for EncoderSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            bitrate_bps: 4_000_000, // 4 Mbps default
            max_fps: 60.0,
        }
    }
}

/// Real-time H.264 video encoder wrapper.
pub struct VideoEncoder {
    encoder: Encoder,
    settings: EncoderSettings,
    width: usize,
    height: usize,
    yuv_buffer: Vec<u8>,
    last_reconfig: std::time::Instant,
}

impl VideoEncoder {
    fn build_config(settings: &EncoderSettings) -> EncoderConfig {
        EncoderConfig::new()
            .bitrate(BitRate::from_bps(settings.bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(settings.max_fps))
            .rate_control_mode(RateControlMode::Bitrate)
            .usage_type(UsageType::ScreenContentRealTime)
            .complexity(Complexity::Low)
            .skip_frames(true)
            .num_threads(4)
            .scene_change_detect(false)
            .background_detection(false)
            .adaptive_quantization(false)
            .vui(VuiConfig::srgb())
    }

    pub fn new(settings: EncoderSettings) -> Result<Self> {
        let mut width = settings.width;
        let mut height = settings.height;
        // OpenH264 requires dimensions to be even numbers
        if width % 2 != 0 { width -= 1; }
        if height % 2 != 0 { height -= 1; }

        let config = Self::build_config(&settings);
        let api = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, config)
            .context("Failed to initialize OpenH264 encoder")?;

        let yuv_size = (3 * width * height) / 2;
        let yuv_buffer = vec![0u8; yuv_size];

        info!(
            "Video encoder initialized: {}x{}, target bitrate {} kbps, max fps {:.1}",
            width, height, settings.bitrate_bps / 1000, settings.max_fps
        );

        Ok(Self {
            encoder,
            settings,
            width,
            height,
            yuv_buffer,
            last_reconfig: std::time::Instant::now(),
        })
    }

    /// Dynamically update target bitrate according to real-time network conditions.
    /// Only reconfigures OpenH264 on significant sustained congestion drops (> 35% reduction)
    /// with a 5-second cooldown to preserve predictive P-frames and prevent IDR keyframe thrashing.
    pub fn set_bitrate(&mut self, bitrate_bps: u32) {
        let current_bps = self.settings.bitrate_bps;
        let is_major_drop = (bitrate_bps as f32) < (current_bps as f32 * 0.65);
        let is_major_jump = (bitrate_bps as f32) > (current_bps as f32 * 1.50);
        let cooldown_passed = self.last_reconfig.elapsed().as_secs() >= 5;

        if (is_major_drop || is_major_jump) && cooldown_passed {
            self.settings.bitrate_bps = bitrate_bps;
            self.last_reconfig = std::time::Instant::now();
            let config = Self::build_config(&self.settings);
            let api = OpenH264API::from_source();
            if let Ok(new_enc) = Encoder::with_api_config(api, config) {
                self.encoder = new_enc;
                self.encoder.force_intra_frame();
                info!("OpenH264 bitrate reconfigured to {} kbps (cooldown elapsed)", bitrate_bps / 1000);
            }
        }
    }

    /// Force the next encoded frame to be an IDR keyframe (with SPS/PPS headers).
    pub fn force_keyframe(&mut self) {
        debug!("Forcing IDR keyframe on next encode");
        self.encoder.force_intra_frame();
    }

    /// Encode a raw BGRx/BGRA frame into H.264 NAL units (supporting scaled resolutions).
    pub fn encode_bgrx(
        &mut self,
        bgrx_data: &[u8],
        src_width: usize,
        src_height: usize,
        stride: usize,
        timestamp_ns: u64,
    ) -> Result<EncodedPacket> {
        let dst_width = self.width;
        let dst_height = self.height;

        // Convert BGRx to I420 YUV (fast LUT scaled if src != dst, or 1:1)
        bgrx_to_yuv420p_scaled(
            bgrx_data,
            src_width,
            src_height,
            stride,
            dst_width,
            dst_height,
            &mut self.yuv_buffer,
        );

        let y_len = dst_width * dst_height;
        let uv_len = (dst_width / 2) * (dst_height / 2);
        let (y_plane, rest) = self.yuv_buffer.split_at(y_len);
        let (u_plane, v_plane) = rest.split_at(uv_len);

        let yuv_source = BorrowedYuv {
            y: y_plane,
            u: u_plane,
            v: v_plane,
            width: dst_width,
            height: dst_height,
        };

        let bitstream = self.encoder.encode(&yuv_source)
            .context("OpenH264 encode failed")?;

        let mut data = Vec::new();
        bitstream.write_vec(&mut data);

        let frame_type = bitstream.frame_type();
        let is_keyframe = matches!(frame_type, FrameType::IDR | FrameType::I);

        Ok(EncodedPacket {
            data,
            is_keyframe,
            timestamp_ns,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

/// Convert BGRx/BGRA pixels (4 bytes per pixel) into I420 YUV planar format (ITU-R BT.601),
/// with fast downscaling if source and destination dimensions differ.
pub fn bgrx_to_yuv420p_scaled(
    bgrx: &[u8],
    src_width: usize,
    src_height: usize,
    stride: usize,
    dst_width: usize,
    dst_height: usize,
    yuv: &mut [u8],
) {
    if dst_width == src_width && dst_height == src_height {
        bgrx_to_yuv420p(bgrx, src_width, src_height, stride, yuv);
        return;
    }

    let y_plane_size = dst_width * dst_height;
    let uv_plane_size = (dst_width / 2) * (dst_height / 2);
    let (y_plane, rest) = yuv.split_at_mut(y_plane_size);
    let (u_plane, v_plane) = rest.split_at_mut(uv_plane_size);

    let half_dst_w = dst_width / 2;
    let half_dst_h = dst_height / 2;

    for y_pair in 0..half_dst_h {
        let dy0 = y_pair * 2;
        let dy1 = dy0 + 1;

        let sy0 = (dy0 * src_height) / dst_height;
        let sy1 = (dy1 * src_height) / dst_height;

        let row0_offset = sy0 * stride;
        let row1_offset = sy1 * stride;

        let y0_dst_offset = dy0 * dst_width;
        let y1_dst_offset = dy1 * dst_width;
        let uv_dst_offset = y_pair * half_dst_w;

        for x_pair in 0..half_dst_w {
            let dx0 = x_pair * 2;
            let dx1 = dx0 + 1;

            let sx0 = (dx0 * src_width) / dst_width;
            let sx1 = (dx1 * src_width) / dst_width;

            let p0 = row0_offset + sx0 * 4;
            let p1 = row0_offset + sx1 * 4;
            let p2 = row1_offset + sx0 * 4;
            let p3 = row1_offset + sx1 * 4;

            if p3 + 2 >= bgrx.len() {
                continue;
            }

            let b0 = bgrx[p0] as i32;
            let g0 = bgrx[p0 + 1] as i32;
            let r0 = bgrx[p0 + 2] as i32;

            let b1 = bgrx[p1] as i32;
            let g1 = bgrx[p1 + 1] as i32;
            let r1 = bgrx[p1 + 2] as i32;

            let b2 = bgrx[p2] as i32;
            let g2 = bgrx[p2 + 1] as i32;
            let r2 = bgrx[p2 + 2] as i32;

            let b3 = bgrx[p3] as i32;
            let g3 = bgrx[p3 + 1] as i32;
            let r3 = bgrx[p3 + 2] as i32;

            y_plane[y0_dst_offset + dx0] = (((66 * r0 + 129 * g0 + 25 * b0 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y0_dst_offset + dx1] = (((66 * r1 + 129 * g1 + 25 * b1 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y1_dst_offset + dx0] = (((66 * r2 + 129 * g2 + 25 * b2 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y1_dst_offset + dx1] = (((66 * r3 + 129 * g3 + 25 * b3 + 128) >> 8) + 16).clamp(16, 235) as u8;

            let r_avg = (r0 + r1 + r2 + r3 + 2) >> 2;
            let g_avg = (g0 + g1 + g2 + g3 + 2) >> 2;
            let b_avg = (b0 + b1 + b2 + b3 + 2) >> 2;

            let uv_idx = uv_dst_offset + x_pair;
            u_plane[uv_idx] = (((-38 * r_avg - 74 * g_avg + 112 * b_avg + 128) >> 8) + 128).clamp(16, 240) as u8;
            v_plane[uv_idx] = (((112 * r_avg - 94 * g_avg - 18 * b_avg + 128) >> 8) + 128).clamp(16, 240) as u8;
        }
    }
}

/// Convert BGRx/BGRA pixels (4 bytes per pixel) into I420 YUV planar format (ITU-R BT.601).
///
/// Output memory layout:
/// - Y plane: width * height bytes
/// - U plane: (width / 2) * (height / 2) bytes
/// - V plane: (width / 2) * (height / 2) bytes
pub fn bgrx_to_yuv420p(
    bgrx: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    yuv: &mut [u8],
) {
    let y_plane_size = width * height;
    let uv_plane_size = (width / 2) * (height / 2);
    let (y_plane, rest) = yuv.split_at_mut(y_plane_size);
    let (u_plane, v_plane) = rest.split_at_mut(uv_plane_size);

    let half_w = width / 2;
    let half_h = height / 2;

    for y_pair in 0..half_h {
        let y0 = y_pair * 2;
        let y1 = y0 + 1;

        let row0_offset = y0 * stride;
        let row1_offset = y1 * stride;

        let y0_offset = y0 * width;
        let y1_offset = y1 * width;
        let uv_offset = y_pair * half_w;

        for x_pair in 0..half_w {
            let x0 = x_pair * 2;
            let x1 = x0 + 1;

            let p0 = row0_offset + x0 * 4;
            let p1 = row0_offset + x1 * 4;
            let p2 = row1_offset + x0 * 4;
            let p3 = row1_offset + x1 * 4;

            if p3 + 2 >= bgrx.len() {
                break;
            }

            let b0 = bgrx[p0] as i32;
            let g0 = bgrx[p0 + 1] as i32;
            let r0 = bgrx[p0 + 2] as i32;

            let b1 = bgrx[p1] as i32;
            let g1 = bgrx[p1 + 1] as i32;
            let r1 = bgrx[p1 + 2] as i32;

            let b2 = bgrx[p2] as i32;
            let g2 = bgrx[p2 + 1] as i32;
            let r2 = bgrx[p2 + 2] as i32;

            let b3 = bgrx[p3] as i32;
            let g3 = bgrx[p3 + 1] as i32;
            let r3 = bgrx[p3 + 2] as i32;

            y_plane[y0_offset + x0] = (((66 * r0 + 129 * g0 + 25 * b0 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y0_offset + x1] = (((66 * r1 + 129 * g1 + 25 * b1 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y1_offset + x0] = (((66 * r2 + 129 * g2 + 25 * b2 + 128) >> 8) + 16).clamp(16, 235) as u8;
            y_plane[y1_offset + x1] = (((66 * r3 + 129 * g3 + 25 * b3 + 128) >> 8) + 16).clamp(16, 235) as u8;

            let r_avg = (r0 + r1 + r2 + r3 + 2) >> 2;
            let g_avg = (g0 + g1 + g2 + g3 + 2) >> 2;
            let b_avg = (b0 + b1 + b2 + b3 + 2) >> 2;

            let uv_idx = uv_offset + x_pair;
            u_plane[uv_idx] = (((-38 * r_avg - 74 * g_avg + 112 * b_avg + 128) >> 8) + 128).clamp(16, 240) as u8;
            v_plane[uv_idx] = (((112 * r_avg - 94 * g_avg - 18 * b_avg + 128) >> 8) + 128).clamp(16, 240) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgrx_to_yuv_conversion() {
        let width = 64;
        let height = 64;
        let stride = width * 4;
        let mut bgrx = vec![0u8; height * stride];

        // Fill with blue color: B=255, G=0, R=0
        for y in 0..height {
            for x in 0..width {
                let idx = y * stride + x * 4;
                bgrx[idx] = 255;
            }
        }

        let mut yuv = vec![0u8; (3 * width * height) / 2];
        bgrx_to_yuv420p(&bgrx, width, height, stride, &mut yuv);

        assert_eq!(yuv.len(), (3 * 64 * 64) / 2);
        // Pure blue in BT.601: Y is approx 41, U is approx 240
        assert!(yuv[0] >= 35 && yuv[0] <= 45, "Y value: {}", yuv[0]);
    }

    #[test]
    fn test_video_encoder_encode() {
        let settings = EncoderSettings {
            width: 128,
            height: 128,
            bitrate_bps: 1_000_000,
            max_fps: 30.0,
        };

        let mut encoder = VideoEncoder::new(settings).expect("Encoder init failed");
        let stride = 128 * 4;
        let bgrx = vec![128u8; 128 * stride];

        // Encode first frame (must be a keyframe)
        let packet = encoder.encode_bgrx(&bgrx, 128, 128, stride, 0)
            .expect("First frame encode failed");

        assert!(packet.is_keyframe, "First encoded frame should be a keyframe");
        assert!(!packet.data.is_empty(), "Packet data should not be empty");

        // Verify H.264 start code (0x00 0x00 0x00 0x01 or 0x00 0x00 0x01)
        assert_eq!(&packet.data[0..3], &[0x00, 0x00, 0x00]);
        assert_eq!(packet.data[3], 0x01);

        // Encode second frame
        let packet2 = encoder.encode_bgrx(&bgrx, 128, 128, stride, 33_333_333)
            .expect("Second frame encode failed");
        assert!(!packet2.data.is_empty(), "Second frame data should not be empty");

        // Force keyframe
        encoder.force_keyframe();
        let packet3 = encoder.encode_bgrx(&bgrx, 128, 128, stride, 66_666_666)
            .expect("Forced keyframe encode failed");
        assert!(packet3.is_keyframe, "Third frame should be forced keyframe");
    }

    #[test]
    fn test_bgrx_to_yuv420p_scaled() {
        let src_w = 192;
        let src_h = 108;
        let dst_w = 128;
        let dst_h = 72;
        let stride = src_w * 4;
        let mut bgrx = vec![0u8; src_h * stride];

        // Fill with green color: B=0, G=255, R=0
        for y in 0..src_h {
            for x in 0..src_w {
                let idx = y * stride + x * 4;
                bgrx[idx + 1] = 255;
            }
        }

        let mut yuv = vec![0u8; (3 * dst_w * dst_h) / 2];
        bgrx_to_yuv420p_scaled(&bgrx, src_w, src_h, stride, dst_w, dst_h, &mut yuv);

        assert_eq!(yuv.len(), (3 * dst_w * dst_h) / 2);
        // Green Y in BT.601 is approx 145
        assert!(yuv[0] >= 140 && yuv[0] <= 150, "Y value: {}", yuv[0]);
    }
}
