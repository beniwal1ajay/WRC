# WARD — Security Model

**Version:** 0.1.0
**Status:** Design

---

## 1. Threat Model

WARD is a remote desktop application. The attack surface is significant:

| Threat                          | Risk   | Mitigation                                        |
|---------------------------------|--------|---------------------------------------------------|
| Network eavesdropping           | HIGH   | TLS 1.3 encryption for all communication          |
| Man-in-the-middle (MITM)       | HIGH   | Device pairing with public key verification       |
| Unauthorized remote control     | HIGH   | Device authentication, pairing code                |
| Replay attacks                  | MEDIUM | Sequence numbers, TLS session binding             |
| Key theft (server)              | MEDIUM | File permissions (0600), optional keyring          |
| Key theft (Android)             | MEDIUM | Android Keystore (hardware-backed if available)   |
| Denial of service               | MEDIUM | Rate limiting, connection limits                  |
| Malformed packet exploitation   | MEDIUM | Strict parsing, bounds checking, Rust memory safety|
| Credential logging              | LOW    | Never log keys, passwords, clipboard, keystrokes  |
| Session hijacking               | MEDIUM | Per-session TLS, device identity binding          |

---

## 2. Encryption

### Transport Encryption

All WARD communication uses **TLS 1.3** over TCP.

| Property              | Value                                      |
|-----------------------|--------------------------------------------|
| Protocol              | TLS 1.3 (RFC 8446)                         |
| Key exchange           | X25519 (ECDHE)                             |
| Authentication         | Ed25519 certificates                       |
| Cipher suites          | TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256 |
| Forward secrecy        | Yes (ephemeral key exchange)               |

### Why TLS 1.3?

1. Mandatory forward secrecy.
2. Reduced handshake latency (1-RTT).
3. No legacy cipher negotiation.
4. Well-audited, battle-tested protocol.
5. Available via OpenSSL 3.x (already installed).

### What is NOT encrypted

Nothing. All data between client and server is encrypted:
- Video frames
- Input events (mouse, keyboard)
- Clipboard content
- Control messages
- Statistics

---

## 3. Authentication

### 3.1 Device Identity

Each device (server and client) has a persistent **Ed25519 keypair**:

- **Server keypair:** Generated on first run, stored at `~/.config/ward/server_key.pem`
- **Client keypair:** Generated on first run, stored in Android Keystore

The Ed25519 public key serves as the **device identity**.

### 3.2 First-Time Pairing

```
Server                                Client
  |                                      |
  | [User initiates pairing]             |
  | Display: "WARD Pairing Code: 847-293"|
  |                                      |
  |<-- TLS connection ------------------|
  |<-- CONNECTION_INIT (client pubkey) --|
  |--- AUTH_REQUEST (method=PAIRING) --->|
  |                                      |
  | [User enters code on Android]        |
  |                                      |
  |<-- AUTH_RESPONSE (pairing code) -----|
  |                                      |
  | [Server verifies code]               |
  |                                      |
  |--- SESSION_ACCEPTED ---------------->|
  | [Server stores client pubkey]        |
  | [Client stores server pubkey]        |
  |                                      |
```

The pairing code is a **6-digit number** displayed on the server.
It is valid for **60 seconds** and can only be used once.

### 3.3 Subsequent Connections (Trusted Device)

```
Server                                Client
  |                                      |
  |<-- TLS connection ------------------|
  |<-- CONNECTION_INIT (client pubkey) --|
  |                                      |
  | [Server checks trusted devices]     |
  |                                      |
  |--- AUTH_REQUEST (challenge) -------->|
  |<-- AUTH_RESPONSE (signed challenge) -|
  |                                      |
  | [Server verifies Ed25519 signature] |
  |                                      |
  |--- SESSION_ACCEPTED ---------------->|
```

The challenge is a **32-byte random nonce** generated per connection.
The client signs it with their Ed25519 private key.
The server verifies using the stored public key.

---

## 4. Key Storage

### Server (Ubuntu)

| Item                | Location                              | Permissions |
|---------------------|---------------------------------------|-------------|
| Server private key  | `~/.config/ward/server_key.pem`       | `0600`      |
| Trusted devices     | `~/.config/ward/devices/`             | `0700` (dir)|
| Device public keys  | `~/.config/ward/devices/{id}.pub`     | `0644`      |
| TLS certificate     | `~/.config/ward/tls_cert.pem`         | `0644`      |
| TLS private key     | `~/.config/ward/tls_key.pem`          | `0600`      |
| Configuration       | `~/.config/ward/config.toml`          | `0600`      |

### Client (Android)

| Item                | Storage                               |
|---------------------|---------------------------------------|
| Client private key  | Android Keystore (hardware-backed)    |
| Server public keys  | EncryptedSharedPreferences            |
| TLS certificates    | Application internal storage          |

---

## 5. Replay Protection

1. **TLS 1.3** provides session-level replay protection.
2. **Sequence numbers** in the WARD protocol header detect duplicate/reordered messages.
3. **Challenge-response authentication** uses fresh random nonces per connection.
4. **Pairing codes** are single-use and time-limited.

---

## 6. Session Management

| Property               | Value                                    |
|------------------------|------------------------------------------|
| Session timeout        | 24 hours (configurable)                  |
| Idle timeout           | 30 minutes (configurable)                |
| Max concurrent clients | 1 (personal use)                         |
| Reconnection           | Within session lifetime                  |

---

## 7. Device Revocation

Trusted devices can be revoked via:

```bash
ward revoke <device-name>
```

This:
1. Removes the device's public key from `~/.config/ward/devices/`.
2. Immediately disconnects the device if connected.
3. Prevents future connections from that device.

---

## 8. Network Exposure

| Property                | Default                                |
|-------------------------|----------------------------------------|
| Listening address       | `0.0.0.0:9740` (LAN only)             |
| mDNS advertisement      | `_ward._tcp.local`                     |
| Firewall                | Port 9740 must be reachable on LAN     |
| No UPnP                 | Server does NOT configure router       |
| No Internet exposure    | LAN only in MVP                        |

### Information Exposed via mDNS

| Exposed              | Not Exposed                      |
|----------------------|----------------------------------|
| Service name "WARD"  | Ubuntu username                  |
| Machine hostname     | Installed software               |
| Port number          | File system contents             |
|                      | Running applications             |

---

## 9. Logging Security

### NEVER log:
- Private keys
- Pairing codes
- Challenge/response values
- Clipboard content
- Keyboard input (keycodes are OK at TRACE level, but NOT the decoded characters)
- Screen content
- Passwords
- Session tokens/secrets

### Safe to log:
- Connection events (connect, disconnect)
- Device names/IDs (public keys are public)
- Error conditions
- Performance statistics
- Protocol state transitions

---

## 10. Attack Surface Analysis

### Network Attack Surface

| Port  | Service          | Authentication Required | Notes          |
|-------|------------------|------------------------|----------------|
| 9740  | WARD server      | Yes                    | TLS 1.3        |
| 5353  | mDNS (Avahi)     | N/A                    | Discovery only |

### Process Attack Surface

| Process     | Privileges       | Notes                                |
|-------------|------------------|--------------------------------------|
| ward-server | User-level       | No root required                     |
|             | D-Bus access     | Portal API only                      |
|             | PipeWire access  | Via portal-granted fd                |

### Data Attack Surface

| Data                | Protection                              |
|---------------------|----------------------------------------|
| Video frames        | TLS encrypted in transit               |
| Input events        | TLS encrypted in transit               |
| Clipboard           | TLS encrypted in transit               |
| Private keys        | File permissions / Keystore            |
| Config              | File permissions                       |

---

## 11. MITM Considerations

### During Pairing

The pairing code provides **out-of-band verification**:
- The code is displayed on the server's physical screen.
- The user visually reads it and enters it on the Android device.
- An attacker would need physical access to see the code.

### After Pairing

- Both devices know each other's public keys.
- The TLS connection uses these keys for authentication.
- An attacker cannot impersonate either device without the private key.

### Limitation

The initial TLS connection during pairing is authenticated only by the server's self-signed certificate. A sophisticated MITM could intercept this. However:
- This requires active network MITM on the LAN.
- The pairing code provides secondary verification.
- After pairing, all subsequent connections are fully authenticated.

---

## 12. Future Security Enhancements

1. **Certificate pinning** after pairing.
2. **TOFU (Trust On First Use)** warnings for certificate changes.
3. **Audit log** of connection attempts.
4. **Rate limiting** for failed authentication.
5. **Automatic key rotation** (periodic keypair refresh).
6. **Secure clipboard** (optional encryption of clipboard content at rest).
