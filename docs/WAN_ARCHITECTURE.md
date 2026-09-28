# WARD — WAN Remote Access Architecture & Modular Separation

**Status:** Phase 1 (Transport Refactor) Complete  
**Date:** 2026-09-28  

---

## 1. Modular Stack Separation

The WARD system is cleanly separated into 8 distinct architectural layers. The core media and control engines (Screen Capture, Video Encoding, Remote Input, and Clipboard) have zero direct dependency on network sockets or transport protocols.

```
┌────────────────────────────────────────────────────────┐
│                    User Experience                     │
│    Android Jetpack Compose UI / Multi-Gesture Trackpad │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────┴────────────────────────────┐
│                  Session Management                    │
│   Client Lifecycle / State Machine / Video Bridging    │
└──────────┬───────────────────────────────┬─────────────┘
           │                               │
┌──────────▼──────────┐         ┌──────────▼─────────────┐
│  Media & Input      │         │ Identity & Security    │
│  • Screen Capture   │         │ • Device Identity      │
│  • Video Encoder    │         │ • Cryptographic Auth   │
│  • Remote Desktop   │         │ • Paired Devices       │
│  • Clipboard Sync   │         │ • Discovery / STUN     │
└──────────┬──────────┘         └──────────┬─────────────┘
           │                               │
┌──────────▼───────────────────────────────▼─────────────┐
│                 Transport Abstraction Layer            │
│               TransportStream / WardTransport          │
├───────────────────────┬─────────────────┬──────────────┤
│      LAN (TLS 1.3)    │   Direct P2P    │ Secure Relay │
│      TCP Port 9740    │   (QUIC / ICE)  │ (Encrypted)  │
└───────────────────────┴─────────────────┴──────────────┘
```

### Module Responsibilities:

1. **Screen Capture (`server/src/capture/`)**:
   - Manages PipeWire DMA-BUF / MemFd stream creation via XDG ScreenCast portal.
   - Outputs raw frame buffers (`CapturedFrame`: BGRx data, stride, dimensions, timestamp).
   - Completely agnostic of how or where frames are transmitted.

2. **Video Encoding (`server/src/encoder/`)**:
   - Encapsulates OpenH264 v0.9 BGRx-to-YUV420p color conversion and H.264 slice compression.
   - Supports dynamic IDR keyframe forcing on demand.
   - Outputs encoded NAL units (`EncodedPacket`).

3. **Remote Input (`server/src/portal/` & `android/ui/`)**:
   - Linux: Maps high-level input actions to `org.freedesktop.portal.RemoteDesktop` D-Bus calls.
   - Android: Captures multi-touch gestures and native `RemoteInputView` keystrokes without buffer desynchronization.
   - Operates strictly on protocol input messages (`MouseMove`, `MouseButton`, `MouseScroll`, `KeyDown`, `KeyUp`).

4. **Clipboard (`server/src/clipboard/` & `android/clipboard/`)**:
   - Interfaces with Wayland clipboard listeners and Android `ClipboardManager`.
   - Uses content hash deduplication to eliminate infinite copy-paste loops.

5. **Authentication (`server/src/auth/` & `android/auth/`)**:
   - Manages Ed25519 device keypairs and trusted device lists.
   - Validates session tokens and enforces mutual authentication.

6. **Transport Abstraction (`server/src/transport/` & `android/transport/`)**:
   - Unified `TransportStream` trait (Rust) and `WardTransport` interface (Kotlin).
   - Implementations for `LAN` (TLS 1.3 TCP), `DirectP2p` (ICE/STUN), and `Relay` (TURN/encrypted relay).
   - Provides bidirectional async streaming, connection health metrics, and round-trip timing.

7. **Connection Management (`server/src/session/` & `android/connection/`)**:
   - Connection selection policy: LAN $\to$ Direct P2P $\to$ Relay fallback.
   - Automatic reconnect and mobile network transition handling.

8. **Discovery (`server/src/discovery/`)**:
   - STUN NAT mapping and ICE candidate gathering.

---

## 2. Transport Abstraction Design

### Rust Server (`TransportStream` & `TransportListener`):
```rust
pub enum TransportType {
    Lan,
    DirectP2p,
    Relay,
}

pub trait TransportStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {
    fn transport_type(&self) -> TransportType;
    fn peer_addr(&self) -> Option<SocketAddr>;
}

pub trait TransportListener: Send + Sync {
    fn local_addr(&self) -> Result<SocketAddr>;
    fn transport_type(&self) -> TransportType;
}
```

### Android Client (`WardTransport`):
```kotlin
enum class TransportType(val displayName: String) {
    LAN("LAN"),
    DIRECT_P2P("Direct P2P"),
    RELAY("Relay")
}

interface WardTransport {
    val transportType: TransportType
    val isConnected: Boolean

    fun connect(host: String, port: Int)
    fun sendInput(msg: WardMessage)
    fun sendKeyPress(keycode: Int)
    fun requestKeyframe()
    fun receiveHeader(): Header
    fun receivePayload(length: Int): ByteArray
    fun disconnect()
}
```

---

## 3. LAN Functionality Guarantee

The existing LAN streaming pipeline (TLS 1.3 self-signed certificate, OpenH264 60fps streaming, multi-gesture trackpad, native input view) remains fully preserved under `LanTransportListener` and `LanTlsTransport`. All existing commands (`cargo test`, `./gradlew test`, and `cargo run -- --stream`) continue to work seamlessly.
