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
| 0x0070  | CANDIDATE_EXCHANGE      | Bidirectional   | ICE candidates for NAT traversal         |
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
### 5.14 CANDIDATE_EXCHANGE (0x0070)

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Candidate Count (u16)       | (Per candidate follows...)    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Candidate Type (u8) | Proto (u8)| IP Length (u16)             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| IP Address (UTF-8, variable)                                  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Port (u16)          |       Priority (u32)          |
|                               |                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Candidate Types:** 1 = Host, 2 = ServerReflexive (STUN), 3 = Relay (TURN)
- **Protocols:** 1 = UDP, 2 = TCP

---

## 6. Connection Lifecycle

```
Client                                    Server
  |                                          |
  |--- Multi-Tier Transport Probe ---------->| (Tier 1 LAN / Tier 2 VPN / Tier 3 P2P / Tier 4 TURN)
  |--- TLS 1.3 handshake -------------------->|
  |<-- TLS 1.3 handshake complete -----------|
  |                                          |
  |--- CONNECTION_INIT --------------------->| (Protocol version, client device ID, public key)
  |<-- AUTH_REQUEST -------------------------| (Auth method: Token challenge or PIN pairing required)
  |--- AUTH_RESPONSE ----------------------->| (Token or 6-digit user-verified PIN)
  |                                          |
  |  [if authenticated]                      |
  |<-- SESSION_ACCEPTED ---------------------| (Auth token for fast subsequent reconnect)
  |<-- VIDEO_CONFIG -------------------------|
  |<-- VIDEO_KEYFRAME ----------------------|
  |<-- VIDEO_FRAME * N ---------------------|
  |                                          |
  |--- MOUSE_MOVE / TOUCH ------------------>|
  |--- MOUSE_BUTTON ------------------------>|
  |--- KEY_DOWN / KEY_UP ------------------->|
  |                                          |
  |<-> PING / PONG (Heartbeat every 2.5s) -->| (Auto-reconnect triggered if silence > 7.5s)
  |<-> CLIPBOARD_SET / GET ----------------->|
  |                                          |
  |--- DISCONNECT -------------------------->|
  |                                          |
  
  [if not authenticated / locked out]
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
| 0x0008 | RATE_LIMIT              | Too many requests / PIN lockout      |

---

## 8. Multi-Tier Transport Selection Hierarchy

When establishing a session from WAN or cellular connections, WARD probes and establishes links along a 4-tier waterfall priority model:

1. **Tier 1 — Direct LAN (`DirectLan`):** Highest performance, sub-millisecond network latency. Direct TLS connection to RFC 1918 private subnet addresses (`192.168.x.x`, `10.x.x.x`, `172.16-31.x.x`).
2. **Tier 2 — Direct VPN (`DirectVpn`):** Direct secure connectivity across Tailscale / WireGuard (`100.64.0.0/10`, `fd00::/8`). Unfiltered, non-relayed end-to-end transport.
3. **Tier 3 — P2P STUN Hole-Punching (`P2pStun`):** Direct peer-to-peer UDP/TCP hole punching through cone and symmetric NAT boundaries using RFC 5389 STUN server reflexive candidate exchange.
4. **Tier 4 — Authenticated TURN Relay (`TurnRelay`):** Fallback relay guaranteed traversal through restrictive enterprise symmetric NATs and carrier firewalls using RFC 5766 TURN data framing and HMAC-SHA1 message integrity.

### Mobile Network Recovery & Heartbeat Engine
- **Heartbeat Interval:** Android client issues `PING (0x0050)` every `2,500 ms`.
- **Heartbeat Timeout:** If no packets or `PONG (0x0051)` received for `7,500 ms` (3 consecutive missed cycles), connection is flagged degraded and auto-reconnect is triggered.
- **Exponential Backoff:** Base delay `500 ms`, exponent `1.8`, maximum delay `15,000 ms`, plus $\pm 20\%$ uniform randomized jitter.
- **Fast Session Resume:** Client presents persisted `authToken` on reconnect to bypass manual PIN approval and immediately transmits `KEYFRAME_REQUEST (0x0013)` for seamless video continuity.

---

## 9. Security Hardening & Zero-Trust Safeguards

- **End-to-End Encryption:** Strict TLS 1.3 encryption across all tiers (LAN, VPN, P2P, and inside TURN relays).
- **Constant-Time Verification:** All authentication tokens, pairing PINs, STUN transaction IDs, and TURN HMAC-SHA1 integrity hashes are verified using constant-time equality comparisons (`subtle::ConstantTimeEq` in Rust, `MessageDigest.isEqual` on Android) to eliminate side-channel timing attacks.
- **Brute-Force PIN Lockout:** Rate limiter enforces maximum 5 consecutive failed PIN attempts with exponential progressive delays and temporary device lockout.
- **Memory Zeroization:** Cryptographic keys and authorization token memory are zeroized upon drop (`zeroize::ZeroizeOnDrop`).
- **Resource Exhaustion Defense:** Max video payload restricted to 16 MB; control messages capped at 64 KB. Over-sized packets trigger instant disconnect.
