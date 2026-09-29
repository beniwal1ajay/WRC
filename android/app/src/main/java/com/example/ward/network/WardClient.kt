package com.example.ward.network

import android.os.Build
import android.util.Log
import com.example.ward.protocol.*
import com.example.ward.transport.LanTlsTransport
import com.example.ward.transport.TransportType
import com.example.ward.transport.WardTransport
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

private const val TAG = "WardClient"

sealed class ConnectionState {
    object Disconnected : ConnectionState()
    object Connecting : ConnectionState()
    data class Connected(
        val host: String,
        val port: Int,
        val width: Int,
        val height: Int,
        val transportType: TransportType = TransportType.LAN,
        val serverDeviceName: String = "AJAY-UBUNTU"
    ) : ConnectionState()
    data class Reconnecting(
        val attempt: Int,
        val delayMs: Long,
        val reason: String
    ) : ConnectionState()
    data class Failed(val error: String) : ConnectionState()
}

/**
 * Calculates exponential backoff delay with random jitter.
 */
fun computeBackoffDelay(
    attempt: Int,
    initialDelayMs: Long = 500L,
    maxDelayMs: Long = 8000L,
    multiplier: Double = 2.0,
    jitterMs: Long = 250L,
    randomFraction: Double = Math.random()
): Long {
    if (attempt <= 1) {
        val jitter = (randomFraction * jitterMs).toLong()
        return (initialDelayMs + jitter).coerceAtMost(maxDelayMs)
    }
    val exp = Math.pow(multiplier, (attempt - 1).toDouble())
    val raw = (initialDelayMs * exp).toLong()
    val capped = raw.coerceAtMost(maxDelayMs)
    val jitter = (randomFraction * jitterMs).toLong()
    return (capped + jitter).coerceAtMost(maxDelayMs + jitterMs)
}

/**
 * High-level WARD remote desktop client.
 * Features:
 * - Decoupled transport abstraction (LAN TLS, VPN, Direct P2P ICE, TURN Relay)
 * - Periodic heartbeat Ping/Pong telemetry and dead-link detection
 * - Resilient auto-reconnect engine with exponential backoff and jitter
 * - Dynamic multi-tier endpoint re-evaluation upon network change
 * - Fast token-based session re-authentication and instant keyframe request
 */
class WardClient(
    private val onStateChanged: (ConnectionState) -> Unit,
    private val onVideoConfig: (width: Int, height: Int, fps: Int) -> Unit,
    private val onVideoFrame: (data: ByteArray, timestampNs: Long, isKeyframe: Boolean) -> Unit,
    private var transport: WardTransport = LanTlsTransport(),
    private val onSessionAccepted: ((WardMessage.SessionAccepted) -> Unit)? = null,
    private val clientDeviceId: ByteArray = ByteArray(32),
    private val authToken: String = "",
    private val onPinRequired: (((String) -> Unit) -> Unit)? = null,
    private val candidateEndpointsProvider: (() -> List<CandidateEndpoint>)? = null,
    private val autoReconnect: Boolean = true,
    private val maxReconnectAttempts: Int = 10,
    private val pingIntervalMs: Long = 2500L,
    private val deadLinkTimeoutMs: Long = 7500L
) {
    private val isConnectingOrConnected = AtomicBoolean(false)
    private val isUserCancelled = AtomicBoolean(false)
    private var workerThread: Thread? = null

    fun connect(host: String, port: Int) {
        if (isConnectingOrConnected.get()) return

        isUserCancelled.set(false)
        isConnectingOrConnected.set(true)

        workerThread = Thread({
            var targetHost = host
            var targetPort = port
            var attempt = 0
            var lastFailureReason = "Initial connection"

            while (isConnectingOrConnected.get() && !isUserCancelled.get()) {
                if (attempt == 0) {
                    onStateChanged(ConnectionState.Connecting)
                } else {
                    val delayMs = computeBackoffDelay(attempt)
                    Log.i(TAG, "Reconnection attempt #$attempt scheduled in ${delayMs}ms ($lastFailureReason)...")
                    onStateChanged(ConnectionState.Reconnecting(attempt, delayMs, lastFailureReason))
                    try {
                        Thread.sleep(delayMs)
                    } catch (_: InterruptedException) {
                        break
                    }
                    if (isUserCancelled.get()) break

                    // Dynamically re-evaluate candidate endpoints across network tiers if available
                    val candidates = candidateEndpointsProvider?.invoke()
                    if (!candidates.isNullOrEmpty()) {
                        try {
                            val best = kotlinx.coroutines.runBlocking {
                                ConnectionSelector.selectBestEndpoint(candidates, probeTimeoutMs = 1200)
                            }
                            if (best != null && best.success) {
                                targetHost = best.endpoint.host
                                targetPort = best.endpoint.port
                                Log.i(TAG, "Dynamic connection selector chose ${best.endpoint.label} ($targetHost:$targetPort)")
                            }
                        } catch (e: Exception) {
                            Log.w(TAG, "Endpoint re-selection probe error: ${e.message}")
                        }
                    }
                }

                try {
                    runSession(targetHost, targetPort)
                    if (isUserCancelled.get()) {
                        break
                    }
                    // Clean server disconnect or EOF without exception
                    attempt++
                    lastFailureReason = "Server closed connection"
                } catch (e: Exception) {
                    if (isUserCancelled.get()) break
                    attempt++
                    lastFailureReason = e.message ?: "Connection lost"
                    Log.w(TAG, "Session interrupted ($lastFailureReason). Scheduling reconnect attempt #$attempt...")
                }

                if (!autoReconnect || (maxReconnectAttempts in 1 until attempt)) {
                    Log.e(TAG, "Reconnect exhausted after $attempt attempts. Failing.")
                    onStateChanged(ConnectionState.Failed(lastFailureReason))
                    break
                }
            }

            isConnectingOrConnected.set(false)
            if (isUserCancelled.get()) {
                onStateChanged(ConnectionState.Disconnected)
            }
        }, "ward-client-worker").apply { start() }
    }

    private fun runSession(host: String, port: Int) {
        Log.i(TAG, "Initiating connection to $host:$port via ${transport.transportType}...")
        transport.connect(host, port)

        // 1. Send CONNECTION_INIT
        val deviceName = "${Build.MANUFACTURER} ${Build.MODEL}"
        val initMsg = WardMessage.ConnectionInit(
            protocolVersion = 1,
            clientType = 1, // Android
            deviceId = clientDeviceId,
            deviceName = deviceName
        )
        transport.sendInput(initMsg)
        Log.i(TAG, "Sent CONNECTION_INIT ($deviceName)")

        // 2. Read handshake response: AUTH_REQUEST, SESSION_ACCEPTED, or SESSION_REJECTED
        var header = transport.receiveHeader()
        var payload = transport.receivePayload(header.payloadLen)

        if (header.type == MessageType.AUTH_REQUEST) {
            val authReq = WardMessage.AuthRequest.parse(payload)
            Log.i(TAG, "Received AUTH_REQUEST: method=${authReq.authMethod}")

            if (authReq.authMethod == 2.toShort()) {
                // Saved token authentication
                Log.i(TAG, "Authenticating with saved token...")
                val resp = WardMessage.AuthResponse(2.toShort(), authToken.toByteArray(Charsets.UTF_8))
                transport.sendInput(resp)
            } else if (authReq.authMethod == 1.toShort()) {
                // First-time PIN pairing challenge
                Log.i(TAG, "PIN pairing required by host")
                val pinFuture = java.util.concurrent.CompletableFuture<String>()
                if (onPinRequired != null) {
                    onPinRequired.invoke { pin ->
                        pinFuture.complete(pin)
                    }
                } else {
                    throw IllegalStateException("PIN required by server but no PIN callback configured")
                }
                val enteredPin = pinFuture.get(60, java.util.concurrent.TimeUnit.SECONDS)
                val resp = WardMessage.AuthResponse(1.toShort(), enteredPin.trim().toByteArray(Charsets.UTF_8))
                transport.sendInput(resp)
            }

            header = transport.receiveHeader()
            payload = transport.receivePayload(header.payloadLen)
        }

        if (header.type == MessageType.SESSION_REJECTED) {
            val rejected = WardMessage.SessionRejected.parse(payload)
            throw IllegalStateException("Session rejected (${rejected.reasonCode}): ${rejected.reasonMessage}")
        }

        if (header.type != MessageType.SESSION_ACCEPTED) {
            throw IllegalStateException("Expected SESSION_ACCEPTED, received ${header.type}")
        }

        val accepted = WardMessage.SessionAccepted.parse(payload)
        Log.i(TAG, "Session accepted via ${transport.transportType}! Session ID = ${accepted.sessionId}, Server = ${accepted.serverDeviceName}")
        onSessionAccepted?.invoke(accepted)

        // 3. Read initial VIDEO_CONFIG
        val headerConfig = transport.receiveHeader()
        val payloadConfig = transport.receivePayload(headerConfig.payloadLen)
        if (headerConfig.type != MessageType.VIDEO_CONFIG) {
            throw IllegalStateException("Expected VIDEO_CONFIG, received ${headerConfig.type}")
        }
        val config = WardMessage.VideoConfig.parse(payloadConfig)
        Log.i(TAG, "Received VIDEO_CONFIG: ${config.width}x${config.height} @ ${config.fpsTarget} fps")

        onVideoConfig(config.width, config.height, config.fpsTarget.toInt())
        onStateChanged(ConnectionState.Connected(
            host = host,
            port = port,
            width = config.width,
            height = config.height,
            transportType = transport.transportType,
            serverDeviceName = accepted.serverDeviceName
        ))

        // Immediately request keyframe to quickly initialize decoder surface upon reconnect
        requestKeyframe()

        // 4. Heartbeat telemetry and dead link detection thread
        val lastDataReceivedMs = AtomicLong(System.currentTimeMillis())
        val sessionActive = AtomicBoolean(true)

        val heartbeatThread = Thread({
            while (sessionActive.get() && !isUserCancelled.get()) {
                try {
                    Thread.sleep(pingIntervalMs)
                    if (!sessionActive.get() || isUserCancelled.get()) break

                    val now = System.currentTimeMillis()
                    val timeSinceLastData = now - lastDataReceivedMs.get()
                    if (timeSinceLastData > deadLinkTimeoutMs) {
                        Log.w(TAG, "Dead link detected: no packets received for ${timeSinceLastData}ms. Aborting transport to reconnect.")
                        transport.disconnect()
                        break
                    }

                    // Send periodic Heartbeat Ping
                    transport.sendInput(WardMessage.Ping(System.nanoTime()))
                } catch (_: InterruptedException) {
                    break
                } catch (e: Exception) {
                    Log.d(TAG, "Heartbeat ping send failed: ${e.message}")
                    break
                }
            }
        }, "ward-heartbeat").apply { isDaemon = true; start() }

        try {
            var frameCount = 0
            var byteCount = 0L
            var lastStatsReport = System.currentTimeMillis()

            while (sessionActive.get() && !isUserCancelled.get()) {
                val msgHeader = transport.receiveHeader()
                val msgPayload = transport.receivePayload(msgHeader.payloadLen)
                lastDataReceivedMs.set(System.currentTimeMillis())
                byteCount += HEADER_SIZE + msgPayload.size

                when (msgHeader.type) {
                    MessageType.VIDEO_KEYFRAME -> {
                        frameCount++
                        val kf = WardMessage.VideoKeyframe.parse(msgPayload)
                        onVideoFrame(kf.data, kf.timestampNs, true)
                    }
                    MessageType.VIDEO_FRAME -> {
                        frameCount++
                        val vf = WardMessage.VideoFrame.parse(msgPayload)
                        onVideoFrame(vf.data, vf.timestampNs, false)
                    }
                    MessageType.VIDEO_CONFIG -> {
                        val newConfig = WardMessage.VideoConfig.parse(msgPayload)
                        onVideoConfig(newConfig.width, newConfig.height, newConfig.fpsTarget.toInt())
                    }
                    MessageType.PONG -> {
                        val pong = WardMessage.Pong.parse(msgPayload)
                        val rttNs = System.nanoTime() - pong.timestampNs
                        val rttMs = if (rttNs > 0) rttNs / 1_000_000 else 0
                        Log.d(TAG, "Heartbeat Pong acknowledged by host. RTT: ${rttMs}ms")
                    }
                    MessageType.DISCONNECT -> {
                        Log.i(TAG, "Server requested disconnect")
                        isUserCancelled.set(true)
                        break
                    }
                    else -> {
                        Log.d(TAG, "Received message type: ${msgHeader.type}")
                    }
                }

                val now = System.currentTimeMillis()
                val elapsed = now - lastStatsReport
                if (elapsed >= 1500) {
                    val fps = ((frameCount * 1000L) / elapsed).toInt().toShort()
                    val bitrateKbps = ((byteCount * 8L) / elapsed).toInt()
                    val statsMsg = WardMessage.Stats(
                        fps = fps,
                        bitrateKbps = bitrateKbps,
                        encodeMs = 0,
                        latencyMs = 20
                    )
                    transport.sendInput(statsMsg)
                    frameCount = 0
                    byteCount = 0L
                    lastStatsReport = now
                }
            }
        } finally {
            sessionActive.set(false)
            heartbeatThread.interrupt()
            try {
                transport.disconnect()
            } catch (_: Exception) {}
        }
    }

    fun sendInput(msg: WardMessage) {
        if (!isConnectingOrConnected.get()) return
        transport.sendInput(msg)
    }

    fun sendKeyPress(keycode: Int) {
        if (!isConnectingOrConnected.get()) return
        transport.sendKeyPress(keycode)
    }

    fun requestKeyframe() {
        try {
            transport.requestKeyframe()
        } catch (e: Exception) {
            Log.d(TAG, "Keyframe request error: ${e.message}")
        }
    }

    fun disconnect() {
        isUserCancelled.set(true)
        isConnectingOrConnected.set(false)
        try {
            transport.disconnect()
        } catch (_: Exception) {}
        workerThread?.interrupt()
        onStateChanged(ConnectionState.Disconnected)
        Log.i(TAG, "Disconnected from WARD server")
    }
}
