//! PipeWire screen capture.
//!
//! Receives video frames from a PipeWire stream set up by the
//! XDG Desktop Portal ScreenCast session.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result};
use tracing::{debug, info, trace, warn};

/// A captured video frame.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Raw pixel data.
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Stride (bytes per row).
    pub stride: u32,
    /// Pixel format.
    pub format: FrameFormat,
    /// Frame sequence number.
    pub sequence: u64,
}

/// Supported pixel formats from PipeWire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FrameFormat {
    BGRx,
    RGBx,
    BGRA,
    RGBA,
    RGB,
    Unknown(u32),
}

#[allow(dead_code)]
impl FrameFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            FrameFormat::BGRx | FrameFormat::RGBx | FrameFormat::BGRA | FrameFormat::RGBA => 4,
            FrameFormat::RGB => 3,
            FrameFormat::Unknown(_) => 4,
        }
    }
}

/// Statistics about the capture stream.
#[derive(Debug, Default)]
pub struct CaptureStats {
    pub frames_received: AtomicU64,
    pub frames_dropped: AtomicU64,
    pub width: AtomicU64,
    pub height: AtomicU64,
}

/// Runs the PipeWire capture loop in a dedicated thread.
pub fn run_pipewire_capture(
    pw_fd: OwnedFd,
    node_id: u32,
    frame_sender: std::sync::mpsc::SyncSender<CapturedFrame>,
    running: Arc<AtomicBool>,
    stats: Arc<CaptureStats>,
) -> Result<std::thread::JoinHandle<Result<()>>> {
    info!("Starting PipeWire capture thread for node {}", node_id);

    let handle = std::thread::Builder::new()
        .name("ward-pw-capture".into())
        .spawn(move || {
            pipewire_capture_loop(pw_fd, node_id, frame_sender, running, stats)
        })
        .context("Failed to spawn PipeWire capture thread")?;

    Ok(handle)
}

/// The actual PipeWire capture loop.
fn pipewire_capture_loop(
    pw_fd: OwnedFd,
    node_id: u32,
    frame_sender: std::sync::mpsc::SyncSender<CapturedFrame>,
    running: Arc<AtomicBool>,
    stats: Arc<CaptureStats>,
) -> Result<()> {
    use pipewire::spa;

    pipewire::init();

    let mainloop = pipewire::main_loop::MainLoop::new(None)
        .context("Failed to create PipeWire main loop")?;
    let context = pipewire::context::Context::new(&mainloop)
        .context("Failed to create PipeWire context")?;

    let core = context
        .connect_fd(pw_fd, None)
        .context("Failed to connect to PipeWire via portal fd")?;

    info!("Connected to PipeWire core");

    // Track format state — we learn width/height/stride from the param_changed callback
    // and from the chunk metadata on each buffer.
    let format_width: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let format_height: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let sequence = Arc::new(AtomicU64::new(0));

    let stream = pipewire::stream::Stream::new(
        &core,
        "ward-capture",
        pipewire::properties::properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        },
    )
    .context("Failed to create PipeWire stream")?;

    // Set up listeners
    let format_w = format_width.clone();
    let format_h = format_height.clone();
    let stats_clone = stats.clone();
    let running_clone = running.clone();
    let sequence_clone = sequence.clone();
    let mainloop_weak = mainloop.downgrade();

    // Recurring 100ms timer to ensure mainloop quits cleanly even if desktop is idle/static
    let timer_running = running.clone();
    let timer_ml_weak = mainloop.downgrade();
    let _shutdown_timer = mainloop.loop_().add_timer(move |_| {
        if !timer_running.load(Ordering::Relaxed) {
            if let Some(ml) = timer_ml_weak.upgrade() {
                ml.quit();
            }
        }
    });
    let _ = _shutdown_timer.update_timer(
        Some(std::time::Duration::from_millis(100)),
        Some(std::time::Duration::from_millis(100)),
    );

    let listener_builder = stream
        .add_local_listener_with_user_data(())
        .state_changed(move |_stream, _user_data, old, new| {
            debug!("PipeWire stream state changed: {:?} -> {:?}", old, new);
        })
        .param_changed(move |_stream, _user_data, param_id, param| {
            if let Some(param) = param {
                if param_id == pipewire::spa::param::ParamType::Format.as_raw() {
                    info!("PipeWire video format negotiated (pod size: {} bytes)", param.size());
                }
            }
        });

    let mut spare_buffer: Option<Vec<u8>> = None;
    let _listener = listener_builder
        .process(move |stream, _user_data| {
            if !running_clone.load(Ordering::Relaxed) {
                if let Some(ml) = mainloop_weak.upgrade() {
                    ml.quit();
                }
                return;
            }

            match stream.dequeue_buffer() {
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();

                    if datas.is_empty() {
                        trace!("Empty buffer received");
                        return;
                    }

                    let data = &mut datas[0];

                    // Extract chunk metadata in a scope so the immutable borrow
                    // on `data` (via `.chunk()`) is released before `.data()`.
                    let (chunk_size, stride) = {
                        let chunk = data.chunk();
                        if chunk.size() == 0 {
                            trace!("Zero-size chunk");
                            return;
                        }
                        (chunk.size() as usize, chunk.stride() as u32)
                    };

                    // Get the raw data pointer (SHM/MemPtr buffers)
                    let raw_data = match data.data() {
                        Some(d) => d,
                        None => {
                            // DMA-BUF — skip for now, will add support later
                            trace!("No data pointer (DMA-BUF), type={:?}", data.type_());
                            stats_clone.frames_dropped.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    };

                    // Determine dimensions from stride and data size.
                    // Assuming 4 bytes per pixel (BGRx/BGRA).
                    let width = if stride > 0 { stride / 4 } else { 0 };
                    let height = if stride > 0 && chunk_size > 0 {
                        (chunk_size as u32) / stride
                    } else {
                        0
                    };

                    if width == 0 || height == 0 {
                        trace!("Cannot determine dimensions: stride={}, size={}", stride, chunk_size);
                        return;
                    }

                    // Update stats
                    format_w.store(width as u64, Ordering::Relaxed);
                    format_h.store(height as u64, Ordering::Relaxed);
                    stats_clone.width.store(width as u64, Ordering::Relaxed);
                    stats_clone.height.store(height as u64, Ordering::Relaxed);

                    let seq = sequence_clone.fetch_add(1, Ordering::Relaxed);

                    // Copy frame data into recycled buffer or new allocation
                    let copy_size = std::cmp::min(chunk_size, raw_data.len());
                    let mut frame_data = spare_buffer.take().unwrap_or_else(Vec::new);
                    if frame_data.len() != copy_size {
                        frame_data.resize(copy_size, 0);
                    }
                    frame_data.copy_from_slice(&raw_data[..copy_size]);

                    let frame = CapturedFrame {
                        data: frame_data,
                        width,
                        height,
                        stride,
                        // GNOME Wayland typically outputs BGRx
                        format: FrameFormat::BGRx,
                        sequence: seq,
                    };

                    match frame_sender.try_send(frame) {
                        Ok(()) => {
                            stats_clone.frames_received.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(dropped)) => {
                            stats_clone.frames_dropped.fetch_add(1, Ordering::Relaxed);
                            trace!("Frame buffer full, dropping frame {}", seq);
                            spare_buffer = Some(dropped.data);
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            warn!("Frame receiver disconnected");
                            if let Some(ml) = mainloop_weak.upgrade() {
                                ml.quit();
                            }
                        }
                    }
                }
                None => {
                    trace!("No buffer available");
                }
            }
        })
        .register()
        .context("Failed to register stream listener")?;

    // Connect the stream to the portal's PipeWire node.
    // Direction::Input = we're consuming (receiving) video from the node.
    //
    // We provide explicit format params requesting SHM (MemPtr) video buffers.
    // Without this, GNOME's ScreenCast delivers DMA-BUF frames which can't be
    // read via data.data() (returns None). By requesting specific SPA video
    // formats, PipeWire will negotiate SHM delivery.
    //
    // Build format parameter pod requesting Raw Video (BGRx/BGRA/RGBx/RGBA).
    let format_pod_bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: vec![
                spa::pod::Property::new(
                    spa::param::format::FormatProperties::MediaType.as_raw(),
                    spa::pod::Value::Id(spa::utils::Id(spa::param::format::MediaType::Video.as_raw())),
                ),
                spa::pod::Property::new(
                    spa::param::format::FormatProperties::MediaSubtype.as_raw(),
                    spa::pod::Value::Id(spa::utils::Id(spa::param::format::MediaSubtype::Raw.as_raw())),
                ),
                spa::pod::Property::new(
                    spa::param::format::FormatProperties::VideoFormat.as_raw(),
                    spa::pod::Value::Choice(spa::pod::ChoiceValue::Id(spa::utils::Choice(
                        spa::utils::ChoiceFlags::empty(),
                        spa::utils::ChoiceEnum::Enum {
                            default: spa::utils::Id(spa::param::video::VideoFormat::BGRx.as_raw()),
                            alternatives: vec![
                                spa::utils::Id(spa::param::video::VideoFormat::BGRx.as_raw()),
                                spa::utils::Id(spa::param::video::VideoFormat::BGRA.as_raw()),
                                spa::utils::Id(spa::param::video::VideoFormat::RGBx.as_raw()),
                                spa::utils::Id(spa::param::video::VideoFormat::RGBA.as_raw()),
                            ],
                        },
                    ))),
                ),
                spa::pod::Property::new(
                    spa::param::format::FormatProperties::VideoFramerate.as_raw(),
                    spa::pod::Value::Choice(spa::pod::ChoiceValue::Fraction(spa::utils::Choice(
                        spa::utils::ChoiceFlags::empty(),
                        spa::utils::ChoiceEnum::Range {
                            default: spa::utils::Fraction { num: 60, denom: 1 },
                            min: spa::utils::Fraction { num: 0, denom: 1 },
                            max: spa::utils::Fraction { num: 60, denom: 1 },
                        },
                    ))),
                ),
                spa::pod::Property::new(
                    spa::param::format::FormatProperties::VideoMaxFramerate.as_raw(),
                    spa::pod::Value::Choice(spa::pod::ChoiceValue::Fraction(spa::utils::Choice(
                        spa::utils::ChoiceFlags::empty(),
                        spa::utils::ChoiceEnum::Range {
                            default: spa::utils::Fraction { num: 60, denom: 1 },
                            min: spa::utils::Fraction { num: 0, denom: 1 },
                            max: spa::utils::Fraction { num: 60, denom: 1 },
                        },
                    ))),
                ),
            ],
        }),
    )
    .context("Failed to serialize SPA format pod")?
    .0
    .into_inner();

    let pod = spa::pod::Pod::from_bytes(&format_pod_bytes)
        .context("Failed to parse serialized SPA format pod")?;

    let mut params = [pod];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pipewire::stream::StreamFlags::AUTOCONNECT
            | pipewire::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    ).context("Failed to connect PipeWire stream")?;

    info!("PipeWire stream connected to node {}, waiting for frames...", node_id);

    // Run the PipeWire main loop (blocks until quit)
    mainloop.run();

    info!("PipeWire capture loop ended");
    Ok(())
}
