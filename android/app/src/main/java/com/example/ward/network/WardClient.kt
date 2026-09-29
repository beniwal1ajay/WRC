package com.example.ward.network

import android.os.Build
import android.util.Log
import com.example.ward.protocol.*
import com.example.ward.transport.LanTlsTransport
import com.example.ward.transport.TransportType
import com.example.ward.transport.WardTransport
import java.util.concurrent.atomic.AtomicBoolean

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
    data class Failed(val error: String) : ConnectionState()
}

/**
 * High-level WARD remote desktop client.
 * Decoupled from transport specifics via the WardTransport abstraction.
 */
class WardClient(
    private val onStateChanged: (ConnectionState) -> Unit,
    private val onVideoConfig: (width: Int, height: Int, fps: Int) -> Unit,
    private val onVideoFrame: (data: ByteArray, timestampNs: Long, isKeyframe: Boolean) -> Unit,
    private var transport: WardTransport = LanTlsTransport(),
    private val onSessionAccepted: ((WardMessage.SessionAccepted) -> Unit)? = null
) {
    private val isRunning = AtomicBoolean(false)

    fun connect(host: String, port: Int) {
        if (isRunning.get()) return

        Thread {
            try {
                onStateChanged(ConnectionState.Connecting)
                Log.i(TAG, "Initiating connection to $host:$port via ${transport.transportType}...")
                transport.connect(host, port)
                isRunning.set(true)

                // 1. Send CONNECTION_INIT
                val deviceName = "${Build.MANUFACTURER} ${Build.MODEL}"
                val initMsg = WardMessage.ConnectionInit(
                    protocolVersion = 1,
                    clientType = 1, // Android
                    deviceId = ByteArray(32),
                    deviceName = deviceName
                )
                transport.sendInput(initMsg)
                Log.i(TAG, "Sent CONNECTION_INIT ($deviceName)")

                // 2. Read SESSION_ACCEPTED
                val headerAccepted = transport.receiveHeader()
                val payloadAccepted = transport.receivePayload(headerAccepted.payloadLen)
                if (headerAccepted.type != MessageType.SESSION_ACCEPTED) {
                    throw IllegalStateException("Expected SESSION_ACCEPTED, received ${headerAccepted.type}")
                }
                val accepted = WardMessage.SessionAccepted.parse(payloadAccepted)
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

                // 4. Main message receiving loop
                while (isRunning.get()) {
                    val header = transport.receiveHeader()
                    val payload = transport.receivePayload(header.payloadLen)

                    when (header.type) {
                        MessageType.VIDEO_KEYFRAME -> {
                            val kf = WardMessage.VideoKeyframe.parse(payload)
                            onVideoFrame(kf.data, kf.timestampNs, true)
                        }
                        MessageType.VIDEO_FRAME -> {
                            val vf = WardMessage.VideoFrame.parse(payload)
                            onVideoFrame(vf.data, vf.timestampNs, false)
                        }
                        MessageType.VIDEO_CONFIG -> {
                            val newConfig = WardMessage.VideoConfig.parse(payload)
                            onVideoConfig(newConfig.width, newConfig.height, newConfig.fpsTarget.toInt())
                        }
                        MessageType.DISCONNECT -> {
                            Log.i(TAG, "Server requested disconnect")
                            break
                        }
                        else -> {
                            Log.d(TAG, "Received message type: ${header.type}")
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Connection error: ${e.message}", e)
                if (isRunning.get()) {
                    onStateChanged(ConnectionState.Failed(e.message ?: "Connection error"))
                }
            } finally {
                disconnect()
            }
        }.start()
    }

    fun sendInput(msg: WardMessage) {
        if (!isRunning.get()) return
        transport.sendInput(msg)
    }

    fun sendKeyPress(keycode: Int) {
        if (!isRunning.get()) return
        transport.sendKeyPress(keycode)
    }

    fun requestKeyframe() {
        transport.requestKeyframe()
    }

    fun disconnect() {
        if (!isRunning.getAndSet(false)) return
        transport.disconnect()
        onStateChanged(ConnectionState.Disconnected)
        Log.i(TAG, "Disconnected from WARD server")
    }
}
