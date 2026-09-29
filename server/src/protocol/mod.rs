//! WARD Protocol implementation.
//!
//! Conforms to protocol/PROTOCOL.md:
//! - 12-byte header: Magic (0x57), Version (0x01), Type (u16), Length (u32), Seq (u32)
//! - Big-endian byte order
//! - Message serialization and deserialization

use std::io::{Cursor, Read};
use anyhow::{Context, Result, bail};

pub const PROTOCOL_MAGIC: u8 = 0x57; // 'W'
pub const PROTOCOL_VERSION: u8 = 0x01;
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024; // 16 MB
pub const MAX_CONTROL_PAYLOAD_SIZE: usize = 64 * 1024; // 64 KB

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    ConnectionInit = 0x0001,
    AuthRequest = 0x0002,
    AuthResponse = 0x0003,
    SessionAccepted = 0x0004,
    SessionRejected = 0x0005,
    VideoFrame = 0x0010,
    VideoKeyframe = 0x0011,
    VideoConfig = 0x0012,
    KeyframeRequest = 0x0013,
    MouseMove = 0x0020,
    MouseButton = 0x0021,
    MouseScroll = 0x0022,
    KeyDown = 0x0030,
    KeyUp = 0x0031,
    ClipboardSet = 0x0040,
    ClipboardGet = 0x0041,
    ClipboardContent = 0x0042,
    Ping = 0x0050,
    Pong = 0x0051,
    Stats = 0x0060,
    CandidateExchange = 0x0070,
    Disconnect = 0x00F0,
    Error = 0x00FF,
}

impl MessageType {
    pub fn from_u16(val: u16) -> Option<Self> {
        match val {
            0x0001 => Some(Self::ConnectionInit),
            0x0002 => Some(Self::AuthRequest),
            0x0003 => Some(Self::AuthResponse),
            0x0004 => Some(Self::SessionAccepted),
            0x0005 => Some(Self::SessionRejected),
            0x0010 => Some(Self::VideoFrame),
            0x0011 => Some(Self::VideoKeyframe),
            0x0012 => Some(Self::VideoConfig),
            0x0013 => Some(Self::KeyframeRequest),
            0x0020 => Some(Self::MouseMove),
            0x0021 => Some(Self::MouseButton),
            0x0022 => Some(Self::MouseScroll),
            0x0030 => Some(Self::KeyDown),
            0x0031 => Some(Self::KeyUp),
            0x0040 => Some(Self::ClipboardSet),
            0x0041 => Some(Self::ClipboardGet),
            0x0042 => Some(Self::ClipboardContent),
            0x0050 => Some(Self::Ping),
            0x0051 => Some(Self::Pong),
            0x0060 => Some(Self::Stats),
            0x0070 => Some(Self::CandidateExchange),
            0x00F0 => Some(Self::Disconnect),
            0x00FF => Some(Self::Error),
            _ => None,
        }
    }
}

/// 12-byte fixed message header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: u8,
    pub version: u8,
    pub msg_type: MessageType,
    pub payload_len: u32,
    pub sequence: u32,
}

impl Header {
    pub const SIZE: usize = 12;

    pub fn new(msg_type: MessageType, payload_len: u32, sequence: u32) -> Self {
        Self {
            magic: PROTOCOL_MAGIC,
            version: PROTOCOL_VERSION,
            msg_type,
            payload_len,
            sequence,
        }
    }

    pub fn serialize(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.magic;
        buf[1] = self.version;
        buf[2..4].copy_from_slice(&(self.msg_type as u16).to_be_bytes());
        buf[4..8].copy_from_slice(&self.payload_len.to_be_bytes());
        buf[8..12].copy_from_slice(&self.sequence.to_be_bytes());
        buf
    }

    pub fn deserialize(buf: &[u8; Self::SIZE]) -> Result<Self> {
        let magic = buf[0];
        if magic != PROTOCOL_MAGIC {
            bail!("Invalid protocol magic: 0x{:02x}, expected 0x{:02x}", magic, PROTOCOL_MAGIC);
        }

        let version = buf[1];
        if version != PROTOCOL_VERSION {
            bail!("Unsupported protocol version: {}, expected {}", version, PROTOCOL_VERSION);
        }

        let raw_type = u16::from_be_bytes([buf[2], buf[3]]);
        let msg_type = MessageType::from_u16(raw_type)
            .context(format!("Unknown message type: 0x{:04x}", raw_type))?;

        let payload_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        if (payload_len as usize) > MAX_PAYLOAD_SIZE {
            bail!("Payload length {} exceeds maximum {}", payload_len, MAX_PAYLOAD_SIZE);
        }

        if msg_type != MessageType::VideoFrame && msg_type != MessageType::VideoKeyframe {
            if (payload_len as usize) > MAX_CONTROL_PAYLOAD_SIZE {
                bail!(
                    "Control message {:?} payload length {} exceeds maximum {}",
                    msg_type,
                    payload_len,
                    MAX_CONTROL_PAYLOAD_SIZE
                );
            }
        }

        let sequence = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);

        Ok(Self {
            magic,
            version,
            msg_type,
            payload_len,
            sequence,
        })
    }
}

/// ICE candidate for peer-to-peer connectivity establishment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IceCandidate {
    pub candidate_type: u8, // 1 = Host, 2 = ServerReflexive, 3 = Relay
    pub protocol: u8,       // 1 = UDP, 2 = TCP
    pub ip: String,
    pub port: u16,
    pub priority: u32,
}

/// Strongly typed protocol messages.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    ConnectionInit {
        protocol_version: u16,
        client_type: u16,
        device_id: [u8; 32],
        device_name: String,
    },
    AuthRequest {
        auth_method: u16,
        challenge: Vec<u8>,
    },
    AuthResponse {
        auth_method: u16,
        response: Vec<u8>,
    },
    SessionAccepted {
        session_id: u32,
        server_device_id: [u8; 32],
        server_device_name: String,
    },
    SessionRejected {
        reason_code: u32,
        reason_message: String,
    },
    VideoConfig {
        width: u16,
        height: u16,
        codec: u16,
        fps_target: u16,
    },
    VideoFrame {
        timestamp_ns: u64,
        data: Vec<u8>,
    },
    VideoKeyframe {
        timestamp_ns: u64,
        data: Vec<u8>,
    },
    KeyframeRequest,
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseButton {
        button: u8,
        state: u8,
    },
    MouseScroll {
        dx: f32,
        dy: f32,
    },
    KeyDown {
        keycode: u32,
    },
    KeyUp {
        keycode: u32,
    },
    ClipboardSet {
        format: u16,
        content: String,
    },
    ClipboardGet,
    ClipboardContent {
        format: u16,
        content: String,
    },
    Ping {
        timestamp_ns: u64,
    },
    Pong {
        timestamp_ns: u64,
    },
    Stats {
        fps: u16,
        bitrate_kbps: u32,
        encode_ms: u16,
        latency_ms: u16,
    },
    CandidateExchange {
        candidates: Vec<IceCandidate>,
    },
    Disconnect,
    Error {
        code: u32,
        message: String,
    },
}

impl Message {
    pub fn msg_type(&self) -> MessageType {
        match self {
            Self::ConnectionInit { .. } => MessageType::ConnectionInit,
            Self::AuthRequest { .. } => MessageType::AuthRequest,
            Self::AuthResponse { .. } => MessageType::AuthResponse,
            Self::SessionAccepted { .. } => MessageType::SessionAccepted,
            Self::SessionRejected { .. } => MessageType::SessionRejected,
            Self::VideoConfig { .. } => MessageType::VideoConfig,
            Self::VideoFrame { .. } => MessageType::VideoFrame,
            Self::VideoKeyframe { .. } => MessageType::VideoKeyframe,
            Self::KeyframeRequest => MessageType::KeyframeRequest,
            Self::MouseMove { .. } => MessageType::MouseMove,
            Self::MouseButton { .. } => MessageType::MouseButton,
            Self::MouseScroll { .. } => MessageType::MouseScroll,
            Self::KeyDown { .. } => MessageType::KeyDown,
            Self::KeyUp { .. } => MessageType::KeyUp,
            Self::ClipboardSet { .. } => MessageType::ClipboardSet,
            Self::ClipboardGet => MessageType::ClipboardGet,
            Self::ClipboardContent { .. } => MessageType::ClipboardContent,
            Self::Ping { .. } => MessageType::Ping,
            Self::Pong { .. } => MessageType::Pong,
            Self::Stats { .. } => MessageType::Stats,
            Self::CandidateExchange { .. } => MessageType::CandidateExchange,
            Self::Disconnect => MessageType::Disconnect,
            Self::Error { .. } => MessageType::Error,
        }
    }

    pub fn serialize_payload(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::ConnectionInit { protocol_version, client_type, device_id, device_name } => {
                buf.extend_from_slice(&protocol_version.to_be_bytes());
                buf.extend_from_slice(&client_type.to_be_bytes());
                buf.extend_from_slice(device_id);
                let name_bytes = device_name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(name_bytes);
            }
            Self::AuthRequest { auth_method, challenge } => {
                buf.extend_from_slice(&auth_method.to_be_bytes());
                buf.extend_from_slice(&(challenge.len() as u16).to_be_bytes());
                buf.extend_from_slice(challenge);
            }
            Self::AuthResponse { auth_method, response } => {
                buf.extend_from_slice(&auth_method.to_be_bytes());
                buf.extend_from_slice(&(response.len() as u16).to_be_bytes());
                buf.extend_from_slice(response);
            }
            Self::SessionAccepted { session_id, server_device_id, server_device_name } => {
                buf.extend_from_slice(&session_id.to_be_bytes());
                buf.extend_from_slice(server_device_id);
                let name_bytes = server_device_name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(name_bytes);
            }
            Self::SessionRejected { reason_code, reason_message } => {
                buf.extend_from_slice(&reason_code.to_be_bytes());
                let msg_bytes = reason_message.as_bytes();
                buf.extend_from_slice(&(msg_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(msg_bytes);
            }
            Self::VideoConfig { width, height, codec, fps_target } => {
                buf.extend_from_slice(&width.to_be_bytes());
                buf.extend_from_slice(&height.to_be_bytes());
                buf.extend_from_slice(&codec.to_be_bytes());
                buf.extend_from_slice(&fps_target.to_be_bytes());
            }
            Self::VideoFrame { timestamp_ns, data } | Self::VideoKeyframe { timestamp_ns, data } => {
                buf.extend_from_slice(&timestamp_ns.to_be_bytes());
                buf.extend_from_slice(data);
            }
            Self::KeyframeRequest => {}
            Self::MouseMove { x, y } => {
                buf.extend_from_slice(&x.to_be_bytes());
                buf.extend_from_slice(&y.to_be_bytes());
            }
            Self::MouseButton { button, state } => {
                buf.push(*button);
                buf.push(*state);
                buf.extend_from_slice(&[0u8, 0u8]); // Reserved u16
            }
            Self::MouseScroll { dx, dy } => {
                buf.extend_from_slice(&dx.to_be_bytes());
                buf.extend_from_slice(&dy.to_be_bytes());
            }
            Self::KeyDown { keycode } | Self::KeyUp { keycode } => {
                buf.extend_from_slice(&keycode.to_be_bytes());
            }
            Self::ClipboardSet { format, content } | Self::ClipboardContent { format, content } => {
                buf.extend_from_slice(&format.to_be_bytes());
                let bytes = content.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
            Self::ClipboardGet => {}
            Self::Ping { timestamp_ns } | Self::Pong { timestamp_ns } => {
                buf.extend_from_slice(&timestamp_ns.to_be_bytes());
            }
            Self::Stats { fps, bitrate_kbps, encode_ms, latency_ms } => {
                buf.extend_from_slice(&fps.to_be_bytes());
                buf.extend_from_slice(&bitrate_kbps.to_be_bytes());
                buf.extend_from_slice(&encode_ms.to_be_bytes());
                buf.extend_from_slice(&latency_ms.to_be_bytes());
            }
            Self::CandidateExchange { candidates } => {
                buf.extend_from_slice(&(candidates.len() as u16).to_be_bytes());
                for c in candidates {
                    buf.push(c.candidate_type);
                    buf.push(c.protocol);
                    let ip_bytes = c.ip.as_bytes();
                    buf.extend_from_slice(&(ip_bytes.len() as u16).to_be_bytes());
                    buf.extend_from_slice(ip_bytes);
                    buf.extend_from_slice(&c.port.to_be_bytes());
                    buf.extend_from_slice(&c.priority.to_be_bytes());
                }
            }
            Self::Disconnect => {}
            Self::Error { code, message } => {
                buf.extend_from_slice(&code.to_be_bytes());
                let msg_bytes = message.as_bytes();
                buf.extend_from_slice(&(msg_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(msg_bytes);
            }
        }
        buf
    }

    pub fn deserialize_payload(msg_type: MessageType, payload: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(payload);
        match msg_type {
            MessageType::ConnectionInit => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let protocol_version = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let client_type = u16::from_be_bytes(u16_buf);

                let mut device_id = [0u8; 32];
                cur.read_exact(&mut device_id)?;

                cur.read_exact(&mut u16_buf)?;
                let name_len = u16::from_be_bytes(u16_buf) as usize;

                let mut name_buf = vec![0u8; name_len];
                cur.read_exact(&mut name_buf)?;
                let device_name = String::from_utf8(name_buf)
                    .context("Invalid UTF-8 in device_name")?;

                Ok(Self::ConnectionInit {
                    protocol_version,
                    client_type,
                    device_id,
                    device_name,
                })
            }
            MessageType::AuthRequest => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let auth_method = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let challenge_len = u16::from_be_bytes(u16_buf) as usize;

                let mut challenge = vec![0u8; challenge_len];
                cur.read_exact(&mut challenge)?;

                Ok(Self::AuthRequest { auth_method, challenge })
            }
            MessageType::AuthResponse => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let auth_method = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let resp_len = u16::from_be_bytes(u16_buf) as usize;

                let mut response = vec![0u8; resp_len];
                cur.read_exact(&mut response)?;

                Ok(Self::AuthResponse { auth_method, response })
            }
            MessageType::SessionAccepted => {
                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                let session_id = u32::from_be_bytes(u32_buf);

                let mut server_device_id = [0u8; 32];
                let mut server_device_name = String::new();

                if cur.read_exact(&mut server_device_id).is_ok() {
                    let mut u16_buf = [0u8; 2];
                    if cur.read_exact(&mut u16_buf).is_ok() {
                        let name_len = u16::from_be_bytes(u16_buf) as usize;
                        let mut name_buf = vec![0u8; name_len];
                        if cur.read_exact(&mut name_buf).is_ok() {
                            server_device_name = String::from_utf8_lossy(&name_buf).to_string();
                        }
                    }
                }

                Ok(Self::SessionAccepted {
                    session_id,
                    server_device_id,
                    server_device_name,
                })
            }
            MessageType::SessionRejected => {
                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                let reason_code = u32::from_be_bytes(u32_buf);

                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let msg_len = u16::from_be_bytes(u16_buf) as usize;

                let mut msg_buf = vec![0u8; msg_len];
                cur.read_exact(&mut msg_buf)?;
                let reason_message = String::from_utf8(msg_buf)
                    .context("Invalid UTF-8 in reason_message")?;

                Ok(Self::SessionRejected { reason_code, reason_message })
            }
            MessageType::VideoConfig => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let width = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let height = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let codec = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let fps_target = u16::from_be_bytes(u16_buf);

                Ok(Self::VideoConfig { width, height, codec, fps_target })
            }
            MessageType::VideoFrame | MessageType::VideoKeyframe => {
                if payload.len() < 8 {
                    bail!("Payload too small for VideoFrame: {} bytes", payload.len());
                }
                let mut ts_buf = [0u8; 8];
                cur.read_exact(&mut ts_buf)?;
                let timestamp_ns = u64::from_be_bytes(ts_buf);

                let mut data = Vec::new();
                cur.read_to_end(&mut data)?;

                if msg_type == MessageType::VideoKeyframe {
                    Ok(Self::VideoKeyframe { timestamp_ns, data })
                } else {
                    Ok(Self::VideoFrame { timestamp_ns, data })
                }
            }
            MessageType::KeyframeRequest => Ok(Self::KeyframeRequest),
            MessageType::MouseMove => {
                let mut f32_buf = [0u8; 4];
                cur.read_exact(&mut f32_buf)?;
                let x = f32::from_be_bytes(f32_buf);

                cur.read_exact(&mut f32_buf)?;
                let y = f32::from_be_bytes(f32_buf);

                Ok(Self::MouseMove { x, y })
            }
            MessageType::MouseButton => {
                let mut byte_buf = [0u8; 1];
                cur.read_exact(&mut byte_buf)?;
                let button = byte_buf[0];

                cur.read_exact(&mut byte_buf)?;
                let state = byte_buf[0];

                Ok(Self::MouseButton { button, state })
            }
            MessageType::MouseScroll => {
                let mut f32_buf = [0u8; 4];
                cur.read_exact(&mut f32_buf)?;
                let dx = f32::from_be_bytes(f32_buf);

                cur.read_exact(&mut f32_buf)?;
                let dy = f32::from_be_bytes(f32_buf);

                Ok(Self::MouseScroll { dx, dy })
            }
            MessageType::KeyDown => {
                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                Ok(Self::KeyDown { keycode: u32::from_be_bytes(u32_buf) })
            }
            MessageType::KeyUp => {
                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                Ok(Self::KeyUp { keycode: u32::from_be_bytes(u32_buf) })
            }
            MessageType::ClipboardSet | MessageType::ClipboardContent => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let format = u16::from_be_bytes(u16_buf);

                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                let content_len = u32::from_be_bytes(u32_buf) as usize;

                let mut content_buf = vec![0u8; content_len];
                cur.read_exact(&mut content_buf)?;
                let content = String::from_utf8(content_buf)
                    .context("Invalid UTF-8 in clipboard content")?;

                if msg_type == MessageType::ClipboardSet {
                    Ok(Self::ClipboardSet { format, content })
                } else {
                    Ok(Self::ClipboardContent { format, content })
                }
            }
            MessageType::ClipboardGet => Ok(Self::ClipboardGet),
            MessageType::Ping => {
                let mut u64_buf = [0u8; 8];
                cur.read_exact(&mut u64_buf)?;
                Ok(Self::Ping { timestamp_ns: u64::from_be_bytes(u64_buf) })
            }
            MessageType::Pong => {
                let mut u64_buf = [0u8; 8];
                cur.read_exact(&mut u64_buf)?;
                Ok(Self::Pong { timestamp_ns: u64::from_be_bytes(u64_buf) })
            }
            MessageType::Stats => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let fps = u16::from_be_bytes(u16_buf);

                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                let bitrate_kbps = u32::from_be_bytes(u32_buf);

                cur.read_exact(&mut u16_buf)?;
                let encode_ms = u16::from_be_bytes(u16_buf);

                cur.read_exact(&mut u16_buf)?;
                let latency_ms = u16::from_be_bytes(u16_buf);

                Ok(Self::Stats { fps, bitrate_kbps, encode_ms, latency_ms })
            }
            MessageType::CandidateExchange => {
                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let count = u16::from_be_bytes(u16_buf) as usize;

                let mut candidates = Vec::with_capacity(count);
                for _ in 0..count {
                    let mut u8_buf = [0u8; 1];
                    cur.read_exact(&mut u8_buf)?;
                    let candidate_type = u8_buf[0];

                    cur.read_exact(&mut u8_buf)?;
                    let protocol = u8_buf[0];

                    cur.read_exact(&mut u16_buf)?;
                    let ip_len = u16::from_be_bytes(u16_buf) as usize;
                    let mut ip_buf = vec![0u8; ip_len];
                    cur.read_exact(&mut ip_buf)?;
                    let ip = String::from_utf8(ip_buf)
                        .context("Invalid UTF-8 in candidate IP")?;

                    cur.read_exact(&mut u16_buf)?;
                    let port = u16::from_be_bytes(u16_buf);

                    let mut u32_buf = [0u8; 4];
                    cur.read_exact(&mut u32_buf)?;
                    let priority = u32::from_be_bytes(u32_buf);

                    candidates.push(IceCandidate {
                        candidate_type,
                        protocol,
                        ip,
                        port,
                        priority,
                    });
                }

                Ok(Self::CandidateExchange { candidates })
            }
            MessageType::Disconnect => Ok(Self::Disconnect),
            MessageType::Error => {
                let mut u32_buf = [0u8; 4];
                cur.read_exact(&mut u32_buf)?;
                let code = u32::from_be_bytes(u32_buf);

                let mut u16_buf = [0u8; 2];
                cur.read_exact(&mut u16_buf)?;
                let msg_len = u16::from_be_bytes(u16_buf) as usize;

                let mut msg_buf = vec![0u8; msg_len];
                cur.read_exact(&mut msg_buf)?;
                let message = String::from_utf8(msg_buf)
                    .context("Invalid UTF-8 in Error message")?;

                Ok(Self::Error { code, message })
            }
        }
    }
}

/// Asynchronous read of a full WARD message from a stream.
pub async fn read_message<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> Result<(Header, Message)> {
    use tokio::io::AsyncReadExt;

    let mut header_buf = [0u8; Header::SIZE];
    reader.read_exact(&mut header_buf).await
        .context("Failed to read message header")?;

    let header = Header::deserialize(&header_buf)?;

    let mut payload = vec![0u8; header.payload_len as usize];
    if header.payload_len > 0 {
        reader.read_exact(&mut payload).await
            .context("Failed to read message payload")?;
    }

    let msg = Message::deserialize_payload(header.msg_type, &payload)?;
    Ok((header, msg))
}

/// Asynchronous write of a full WARD message to a stream.
pub async fn write_message<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    message: &Message,
    sequence: u32,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let payload = message.serialize_payload();
    let header = Header::new(message.msg_type(), payload.len() as u32, sequence);
    let header_bytes = header.serialize();

    writer.write_all(&header_bytes).await
        .context("Failed to write message header")?;
    if !payload.is_empty() {
        writer.write_all(&payload).await
            .context("Failed to write message payload")?;
    }
    writer.flush().await
        .context("Failed to flush message")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(MessageType::VideoFrame, 1234, 42);
        let bytes = header.serialize();
        let parsed = Header::deserialize(&bytes).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_video_config_roundtrip() {
        let msg = Message::VideoConfig {
            width: 1920,
            height: 1080,
            codec: 1,
            fps_target: 30,
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_mouse_move_roundtrip() {
        let msg = Message::MouseMove { x: 500.5, y: 300.25 };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_mouse_button_roundtrip() {
        let msg = Message::MouseButton { button: 1, state: 1 };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_keyboard_roundtrip() {
        let msg = Message::KeyDown { keycode: 30 };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_connection_init_roundtrip() {
        let msg = Message::ConnectionInit {
            protocol_version: 1,
            client_type: 1,
            device_id: [0x42; 32],
            device_name: "Pixel 8 Pro".to_string(),
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_video_frame_roundtrip() {
        let msg = Message::VideoKeyframe {
            timestamp_ns: 1234567890,
            data: vec![0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00],
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_clipboard_set_roundtrip() {
        let msg = Message::ClipboardSet {
            format: 1,
            content: "Hello from WARD clipboard sync!".to_string(),
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_stats_roundtrip() {
        let msg = Message::Stats {
            fps: 60,
            bitrate_kbps: 4500,
            encode_ms: 3,
            latency_ms: 12,
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[test]
    fn test_candidate_exchange_roundtrip() {
        let msg = Message::CandidateExchange {
            candidates: vec![
                IceCandidate {
                    candidate_type: 1, // Host
                    protocol: 1,       // UDP
                    ip: "192.168.1.100".to_string(),
                    port: 51820,
                    priority: 2130706431,
                },
                IceCandidate {
                    candidate_type: 2, // Srflx
                    protocol: 1,       // UDP
                    ip: "203.0.113.50".to_string(),
                    port: 62411,
                    priority: 1694498815,
                },
            ],
        };
        let payload = msg.serialize_payload();
        let parsed = Message::deserialize_payload(msg.msg_type(), &payload).unwrap();
        assert_eq!(msg, parsed);
    }

    #[tokio::test]
    async fn test_async_read_write_message() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let msg = Message::MouseMove { x: 1920.0, y: 1080.0 };

        let write_fut = write_message(&mut client, &msg, 7);
        let read_fut = read_message(&mut server);

        let (_, (header, received)) = tokio::try_join!(write_fut, read_fut).unwrap();
        assert_eq!(header.sequence, 7);
        assert_eq!(header.msg_type, MessageType::MouseMove);
        assert_eq!(msg, received);
    }

    #[test]
    fn test_header_payload_size_security_limits() {
        // Control message with excessive payload length (> 64KB) must be rejected
        let mut bad_control_buf = [0u8; Header::SIZE];
        bad_control_buf[0] = PROTOCOL_MAGIC;
        bad_control_buf[1] = PROTOCOL_VERSION;
        bad_control_buf[2..4].copy_from_slice(&(MessageType::Ping as u16).to_be_bytes());
        bad_control_buf[4..8].copy_from_slice(&(65537u32).to_be_bytes()); // 64KB + 1 byte
        bad_control_buf[8..12].copy_from_slice(&1u32.to_be_bytes());

        let res = Header::deserialize(&bad_control_buf);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("exceeds maximum"));

        // Video frame with 1MB payload should be permitted
        let mut valid_video_buf = [0u8; Header::SIZE];
        valid_video_buf[0] = PROTOCOL_MAGIC;
        valid_video_buf[1] = PROTOCOL_VERSION;
        valid_video_buf[2..4].copy_from_slice(&(MessageType::VideoFrame as u16).to_be_bytes());
        valid_video_buf[4..8].copy_from_slice(&(1024 * 1024u32).to_be_bytes()); // 1 MB
        valid_video_buf[8..12].copy_from_slice(&2u32.to_be_bytes());

        let res_video = Header::deserialize(&valid_video_buf);
        assert!(res_video.is_ok());

        // Video frame exceeding 16MB must be rejected
        let mut bad_video_buf = [0u8; Header::SIZE];
        bad_video_buf[0] = PROTOCOL_MAGIC;
        bad_video_buf[1] = PROTOCOL_VERSION;
        bad_video_buf[2..4].copy_from_slice(&(MessageType::VideoFrame as u16).to_be_bytes());
        bad_video_buf[4..8].copy_from_slice(&(17 * 1024 * 1024u32).to_be_bytes()); // 17 MB

        let res_too_large = Header::deserialize(&bad_video_buf);
        assert!(res_too_large.is_err());
    }
}

