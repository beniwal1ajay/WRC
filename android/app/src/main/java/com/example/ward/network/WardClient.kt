package com.example.ward.network

import android.os.Build
import android.util.Log
import com.example.ward.protocol.*
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

private const val TAG = "WardClient"

sealed class ConnectionState {
    object Disconnected : ConnectionState()
    object Connecting : ConnectionState()
    data class Connected(val host: String, val port: Int, val width: Int, val height: Int) : ConnectionState()
    data class Failed(val error: String) : ConnectionState()
}

class WardClient(
    private val onStateChanged: (ConnectionState) -> Unit,
    private val onVideoConfig: (width: Int, height: Int, fps: Int) -> Unit,
    private val onVideoFrame: (data: ByteArray, timestampNs: Long, isKeyframe: Boolean) -> Unit
) {
    private var sslSocket: SSLSocket? = null
    private var outputStream: BufferedOutputStream? = null
    private val isRunning = AtomicBoolean(false)
    private val sequence = AtomicInteger(1)
    private val sendLock = Any()
    private val inputQueue = java.util.concurrent.LinkedBlockingQueue<WardMessage>()
    private var senderThread: Thread? = null

    fun connect(host: String, port: Int) {
        if (isRunning.get()) return

        Thread {
            try {
                onStateChanged(ConnectionState.Connecting)
                Log.i(TAG, "Connecting to WARD server at $host:$port (TLS 1.3)...")

                // Create SSLContext trusting self-signed cert on LAN
                val trustAllCerts = arrayOf<TrustManager>(object : X509TrustManager {
                    override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
                    override fun checkClientTrusted(certs: Array<X509Certificate>, authType: String) {}
                    override fun checkServerTrusted(certs: Array<X509Certificate>, authType: String) {}
                })

                val sslContext = SSLContext.getInstance("TLSv1.3").apply {
                    init(null, trustAllCerts, SecureRandom())
                }

                val socket = sslContext.socketFactory.createSocket() as SSLSocket
                socket.tcpNoDelay = true
                socket.soTimeout = 0 // Infinite read timeout for streaming
                socket.connect(InetSocketAddress(host, port), 5000)
                socket.startHandshake()

                Log.i(TAG, "TLS handshake successful. Protocol: ${socket.session.protocol}, Cipher: ${socket.session.cipherSuite}")

                val inStream = BufferedInputStream(socket.inputStream, 65536)
                val outStream = BufferedOutputStream(socket.outputStream, 16384)

                synchronized(sendLock) {
                    sslSocket = socket
                    outputStream = outStream
                }
                isRunning.set(true)
                inputQueue.clear()

                // Dedicated sequential FIFO input sender thread
                senderThread = Thread({
                    Log.i(TAG, "Input sender thread started")
                    while (isRunning.get()) {
                        try {
                            val msg = inputQueue.poll(100, java.util.concurrent.TimeUnit.MILLISECONDS) ?: continue
                            synchronized(sendLock) {
                                outputStream?.let { out ->
                                    val seq = sequence.getAndIncrement()
                                    writeMessage(out, msg, seq)
                                }
                            }
                        } catch (e: InterruptedException) {
                            break
                        } catch (e: Exception) {
                            Log.w(TAG, "Input sender write error: ${e.message}")
                        }
                    }
                    Log.i(TAG, "Input sender thread stopped")
                }, "Ward-InputSender").apply { start() }

                // 1. Send CONNECTION_INIT
                val deviceName = "${Build.MANUFACTURER} ${Build.MODEL}"
                val initMsg = WardMessage.ConnectionInit(
                    protocolVersion = 1,
                    clientType = 1, // Android
                    deviceId = ByteArray(32),
                    deviceName = deviceName
                )
                val seq = sequence.getAndIncrement()
                writeMessage(outStream, initMsg, seq)
                Log.i(TAG, "Sent CONNECTION_INIT ($deviceName)")

                // 2. Read SESSION_ACCEPTED
                val headerAccepted = readHeader(inStream)
                val payloadAccepted = readPayload(inStream, headerAccepted.payloadLen)
                if (headerAccepted.type != MessageType.SESSION_ACCEPTED) {
                    throw IllegalStateException("Expected SESSION_ACCEPTED, received ${headerAccepted.type}")
                }
                val accepted = WardMessage.SessionAccepted.parse(payloadAccepted)
                Log.i(TAG, "Session accepted! Session ID = ${accepted.sessionId}")

                // 3. Read initial VIDEO_CONFIG
                val headerConfig = readHeader(inStream)
                val payloadConfig = readPayload(inStream, headerConfig.payloadLen)
                if (headerConfig.type != MessageType.VIDEO_CONFIG) {
                    throw IllegalStateException("Expected VIDEO_CONFIG, received ${headerConfig.type}")
                }
                val config = WardMessage.VideoConfig.parse(payloadConfig)
                Log.i(TAG, "Received VIDEO_CONFIG: ${config.width}x${config.height} @ ${config.fpsTarget} fps")

                onVideoConfig(config.width, config.height, config.fpsTarget.toInt())
                onStateChanged(ConnectionState.Connected(host, port, config.width, config.height))

                // 4. Main message receiving loop
                while (isRunning.get()) {
                    val header = readHeader(inStream)
                    val payload = readPayload(inStream, header.payloadLen)

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
        inputQueue.offer(msg)
    }

    fun sendKeyPress(keycode: Int) {
        if (!isRunning.get()) return
        inputQueue.offer(WardMessage.KeyDown(keycode))
        inputQueue.offer(WardMessage.KeyUp(keycode))
    }

    fun requestKeyframe() {
        sendInput(WardMessage.KeyframeRequest)
    }

    fun disconnect() {
        if (!isRunning.getAndSet(false)) return

        senderThread?.interrupt()
        senderThread = null
        inputQueue.clear()

        try {
            synchronized(sendLock) {
                outputStream?.let { out ->
                    try {
                        val seq = sequence.getAndIncrement()
                        writeMessage(out, WardMessage.Disconnect, seq)
                    } catch (_: Exception) {}
                }
                sslSocket?.close()
                sslSocket = null
                outputStream = null
            }
        } catch (_: Exception) {}

        onStateChanged(ConnectionState.Disconnected)
        Log.i(TAG, "Disconnected from WARD server")
    }
}
