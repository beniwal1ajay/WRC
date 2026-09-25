# WARD Protocol Specification

**Version:** 0.1.0
**Status:** Draft

---

## 1. Overview

The WARD protocol is a binary, versioned protocol for communication between the WARD server (Ubuntu) and WARD client (Android).

All communication occurs over a TLS 1.3 encrypted TCP connection.

---

## 2. Transport

| Property          | Value                          |
|-------------------|--------------------------------|
| Transport         | TCP                            |
| Encryption        | TLS 1.3                        |
| Default port      | 9740                           |
| Byte order        | Big-endian (network byte order)|

---

## 3. Message Format

Every message has a fixed 12-byte header followed by a variable-length payload.

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Magic (0x57)  |  Version     |         Message Type          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Payload Length                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Sequence Number                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
|                          Payload                              |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Header Fields

| Field           | Offset | Size  | Description                                    |
|-----------------|--------|-------|------------------------------------------------|
| Magic           | 0      | 1     | Always `0x57` ('W' for WARD)                   |
| Version         | 1      | 1     | Protocol version (currently `0x01`)            |
| Message Type    | 2      | 2     | Message type (see §4)                          |
| Payload Length  | 4      | 4     | Length of payload in bytes (0 for no payload)  |
| Sequence Number | 8      | 4     | Monotonically increasing per-sender counter    |

### Constraints

- Maximum payload length: 16 MB (16,777,216 bytes)
- Magic byte mismatch → connection MUST be terminated
- Version mismatch → send `ERROR` message and disconnect
- Unknown message type → send `ERROR` and ignore

---

## 4. Message Types

| Type ID | Name                    | Direction       | Description                              |
|---------|-------------------------|-----------------|------------------------------------------|
| 0x0001  | CONNECTION_INIT         | Client → Server | Initial connection handshake             |
| 0x0002  | AUTH_REQUEST            | Server → Client | Server requests authentication           |
| 0x0003  | AUTH_RESPONSE           | Client → Server | Client provides auth credentials         |
| 0x0004  | SESSION_ACCEPTED        | Server → Client | Authentication successful                |
| 0x0005  | SESSION_REJECTED        | Server → Client | Authentication failed                    |
| 0x0010  | VIDEO_FRAME             | Server → Client | Encoded video frame (P-frame)            |
| 0x0011  | VIDEO_KEYFRAME          | Server → Client | Encoded video keyframe (I-frame)         |
| 0x0012  | VIDEO_CONFIG            | Server → Client | Video configuration (codec, resolution)  |
| 0x0013  | KEYFRAME_REQUEST        | Client → Server | Client requests a keyframe               |
| 0x0020  | MOUSE_MOVE              | Client → Server | Mouse movement (absolute coordinates)    |
| 0x0021  | MOUSE_BUTTON            | Client → Server | Mouse button press/release               |
| 0x0022  | MOUSE_SCROLL            | Client → Server | Mouse scroll event                       |
| 0x0030  | KEY_DOWN                | Client → Server | Keyboard key press                       |
| 0x0031  | KEY_UP                  | Client → Server | Keyboard key release                     |
| 0x0040  | CLIPBOARD_SET           | Bidirectional   | Set clipboard content                    |
| 0x0041  | CLIPBOARD_GET           | Bidirectional   | Request clipboard content                |
| 0x0042  | CLIPBOARD_CONTENT       | Bidirectional   | Clipboard content response               |
| 0x0050  | PING                    | Bidirectional   | Latency measurement request              |
| 0x0051  | PONG                    | Bidirectional   | Latency measurement response             |
| 0x0060  | STATS                   | Server → Client | Server statistics (FPS, bitrate, etc.)   |
| 0x00F0  | DISCONNECT              | Bidirectional   | Clean disconnection                      |
| 0x00FF  | ERROR                   | Bidirectional   | Error notification                       |

---

## 5. Message Payloads

### 5.1 CONNECTION_INIT (0x0001)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Protocol Version (u16)       |  Client Type (u16)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Device ID (32 bytes)                     |
|                            ...                                |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Device Name Length (u16)     |  Device Name (UTF-8)...       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Protocol Version:** Client's supported protocol version
- **Client Type:** 1 = Android
- **Device ID:** 32-byte Ed25519 public key
- **Device Name:** Human-readable device name

### 5.2 AUTH_REQUEST (0x0002)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Auth Method (u16)            |  Challenge Length (u16)        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Challenge (variable)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Auth Method:** 1 = Pairing code, 2 = Trusted device (signed challenge)

### 5.3 AUTH_RESPONSE (0x0003)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Auth Method (u16)            |  Response Length (u16)         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Response (variable)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- For pairing: 6-digit code as UTF-8
- For trusted device: Ed25519 signature over challenge

### 5.4 VIDEO_FRAME / VIDEO_KEYFRAME (0x0010 / 0x0011)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Timestamp (u64, ns)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Encoded Data (variable)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 5.5 VIDEO_CONFIG (0x0012)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Width (u16)         |          Height (u16)         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Codec (u16)                |    FPS Target (u16)           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Codec:** 1 = H.264

### 5.6 MOUSE_MOVE (0x0020)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|              X (f32, IEEE 754)                                |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|              Y (f32, IEEE 754)                                |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

Coordinates are normalized to the video stream resolution (0.0 to width/height).

### 5.7 MOUSE_BUTTON (0x0021)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Button (u8)    |   State (u8)   |     Reserved (u16)        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Button:** 1=Left, 2=Right, 3=Middle
- **State:** 0=Released, 1=Pressed

### 5.8 MOUSE_SCROLL (0x0022)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           dx (f32)                                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           dy (f32)                                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 5.9 KEY_DOWN / KEY_UP (0x0030 / 0x0031)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Keycode (u32, evdev)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 5.10 CLIPBOARD_SET (0x0040)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Format (u16)    |  Content Length (u32)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Content (UTF-8, variable)                   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Format:** 1 = UTF-8 text
- Maximum content length: 1 MB

### 5.11 PING / PONG (0x0050 / 0x0051)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Timestamp (u64, ns)                        |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 5.12 STATS (0x0060)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   FPS (u16)     |   Bitrate kbps (u32)                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Encode ms (u16) |  Latency ms (u16)                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 5.13 ERROR (0x00FF)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Error Code (u32)                                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Message Length (u16)        | Message (UTF-8)...            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

---

## 6. Connection Lifecycle

```
Client                                    Server
  |                                          |
  |--- TLS handshake ----------------------->|
  |<-- TLS handshake complete ---------------|
  |                                          |
  |--- CONNECTION_INIT --------------------->|
  |<-- AUTH_REQUEST -------------------------|
  |--- AUTH_RESPONSE ----------------------->|
  |                                          |
  |  [if authenticated]                      |
  |<-- SESSION_ACCEPTED ---------------------|
  |<-- VIDEO_CONFIG -------------------------|
  |<-- VIDEO_KEYFRAME ----------------------|
  |<-- VIDEO_FRAME * N ---------------------|
  |                                          |
  |--- MOUSE_MOVE ------------------------->|
  |--- MOUSE_BUTTON ----------------------->|
  |--- KEY_DOWN --------------------------->|
  |--- KEY_UP ------------------------------>|
  |                                          |
  |<-> PING / PONG (periodic) ------------->|
  |<-> CLIPBOARD_SET ---------------------->|
  |                                          |
  |--- DISCONNECT ------------------------->|
  |                                          |
  
  [if not authenticated]
  |<-- SESSION_REJECTED ---------------------|
  | (connection closed)                      |
```

---

## 7. Error Codes

| Code   | Name                     | Description                         |
|--------|--------------------------|-------------------------------------|
| 0x0001 | PROTOCOL_MISMATCH       | Unsupported protocol version        |
| 0x0002 | AUTH_FAILED             | Authentication failure               |
| 0x0003 | SESSION_EXPIRED         | Session timed out                    |
| 0x0004 | INVALID_MESSAGE         | Malformed message received           |
| 0x0005 | UNSUPPORTED_CODEC       | Requested codec not available        |
| 0x0006 | INTERNAL_ERROR          | Server internal error                |
| 0x0007 | DEVICE_REVOKED          | Device has been revoked              |
| 0x0008 | RATE_LIMIT              | Too many requests                    |

---

## 8. Security Considerations

- All messages are encrypted via TLS 1.3.
- Sequence numbers prevent replay attacks within a session.
- The CONNECTION_INIT includes the device's Ed25519 public key for identity.
- Authentication uses challenge-response with Ed25519 signatures.
- Maximum payload size prevents memory exhaustion attacks.
- Invalid messages result in ERROR + connection termination.
