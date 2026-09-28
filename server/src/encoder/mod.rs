//! H.264 Video Encoder implementation using OpenH264.
//!
//! Encodes raw frames (BGRx/BGRA) from PipeWire screen capture
//! into standard H.264 Annex B NAL units (I-frames, P-frames) for network streaming.

use anyhow::{Context, Result};
use openh264::OpenH264API;
use openh264::encoder::{Encoder, EncoderConfig, FrameType};
use openh264::formats::YUVBuffer;
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
    width: usize,
    height: usize,
    yuv_buffer: Vec<u8>,
}

impl VideoEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        let mut width = settings.width;
        let mut height = settings.height;
        // OpenH264 requires dimensions to be even numbers
        if width % 2 != 0 { width -= 1; }
        if height % 2 != 0 { height -= 1; }

        let config = EncoderConfig::new();
        let api = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, config)
            .context("Failed to initialize OpenH264 encoder")?;

        let yuv_size = (3 * width * height) / 2;
        let yuv_buffer = vec![0u8; yuv_size];

        info!(
            "Video encoder initialized: {}x{}, target bitrate {} kbps",
            width, height, settings.bitrate_bps / 1000
        );

        Ok(Self {
            encoder,
            width,
            height,
            yuv_buffer,
        })
    }

    /// Force the next encoded frame to be an IDR keyframe (with SPS/PPS headers).
    pub fn force_keyframe(&mut self) {
        debug!("Forcing IDR keyframe on next encode");
        self.encoder.force_intra_frame();
    }

    /// Encode a raw BGRx/BGRA frame into H.264 NAL units.
    pub fn encode_bgrx(
        &mut self,
        bgrx_data: &[u8],
        src_width: usize,
        src_height: usize,
        stride: usize,
        timestamp_ns: u64,
    ) -> Result<EncodedPacket> {
        // Adjust encoder dimensions if screen resolution changed
        if src_width != self.width || src_height != self.height {
            let mut new_w = src_width;
            let mut new_h = src_height;
            if new_w % 2 != 0 { new_w -= 1; }
            if new_h % 2 != 0 { new_h -= 1; }
            self.width = new_w;
            self.height = new_h;
            self.yuv_buffer.resize((3 * new_w * new_h) / 2, 0);
            info!("Encoder resized to {}x{}", new_w, new_h);
        }

        // Convert BGRx to I420 YUV
        bgrx_to_yuv420p(
            bgrx_data,
            self.width,
            self.height,
            stride,
            &mut self.yuv_buffer,
        );

        let yuv_source = YUVBuffer::from_vec(
            self.yuv_buffer.clone(),
            self.width,
            self.height,
        );

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
    let u_plane_size = (width / 2) * (height / 2);
    let (y_plane, rest) = yuv.split_at_mut(y_plane_size);
    let (u_plane, v_plane) = rest.split_at_mut(u_plane_size);

    for y in 0..height {
        let row_start = y * stride;
        let y_row_start = y * width;
        let is_even_row = (y & 1) == 0;
        let uv_row_start = (y / 2) * (width / 2);

        for x in 0..width {
            let pixel_idx = row_start + x * 4;
            if pixel_idx + 2 >= bgrx.len() {
                break;
            }

            let b = bgrx[pixel_idx] as i32;
            let g = bgrx[pixel_idx + 1] as i32;
            let r = bgrx[pixel_idx + 2] as i32;

            // Y = ((66 * R + 129 * G + 25 * B + 128) >> 8) + 16
            let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[y_row_start + x] = y_val.clamp(16, 235) as u8;

            // Compute U and V for 2x2 blocks on even row/col
            if is_even_row && (x & 1) == 0 {
                let uv_idx = uv_row_start + (x / 2);
                if uv_idx < u_plane.len() && uv_idx < v_plane.len() {
                    // U = ((-38 * R - 74 * G + 112 * B + 128) >> 8) + 128
                    let u_val = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                    // V = ((112 * R - 94 * G - 18 * B + 128) >> 8) + 128
                    let v_val = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;

                    u_plane[uv_idx] = u_val.clamp(16, 240) as u8;
                    v_plane[uv_idx] = v_val.clamp(16, 240) as u8;
                }
            }
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
}
