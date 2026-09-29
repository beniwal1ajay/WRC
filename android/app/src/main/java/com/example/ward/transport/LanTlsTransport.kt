package com.example.ward.transport

import android.util.Log
import com.example.ward.protocol.*
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509ExtendedTrustManager

private const val TAG = "LanTlsTransport"

/**
 * LAN Transport implementing TLS 1.3 over direct TCP socket with sequential FIFO input queue.
 */
class LanTlsTransport : WardTransport {
    override val transportType: TransportType = TransportType.LAN

    private var sslSocket: SSLSocket? = null
    private var rawSocket: Socket? = null
    private var inputStream: BufferedInputStream? = null
    private var outputStream: BufferedOutputStream? = null
    private val isRunning = AtomicBoolean(false)
    private val sequence = AtomicInteger(1)
    private val sendLock = Any()
    private val inputQueue = LinkedBlockingQueue<WardMessage>()
    private var senderThread: Thread? = null

    override val isConnected: Boolean
        get() = isRunning.get() && sslSocket?.isConnected == true

    override fun connect(host: String, port: Int) {
        if (isRunning.get()) return

        Log.i(TAG, "[TLS DIAG] Starting LAN TLS connection to $host:$port...")

        // Robust ExtendedTrustManager covering all X509 validation callbacks (standard, socket-based, engine-based)
        val trustAllCerts = arrayOf<TrustManager>(object : X509ExtendedTrustManager() {
            override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {}
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?, socket: Socket?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?, socket: Socket?) {}
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?, engine: SSLEngine?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?, engine: SSLEngine?) {}
        })

        // Use "TLS" as standard context protocol (supports TLS 1.2 and 1.3 across all Android versions)
        val sslContext = try {
            SSLContext.getInstance("TLS").apply {
                init(null, trustAllCerts, SecureRandom())
            }
        } catch (e: Exception) {
            Log.w(TAG, "[TLS DIAG] SSLContext('TLS') init failed, falling back to default: ${e.message}")
            SSLContext.getInstance("Default").apply {
                init(null, trustAllCerts, SecureRandom())
            }
        }

        // Step 1: Connect raw TCP socket first with explicit timeout
        Log.i(TAG, "[TLS DIAG] Step 1: Establishing raw TCP socket to $host:$port...")
        val tcpSocket = Socket()
        tcpSocket.tcpNoDelay = true
        tcpSocket.keepAlive = true
        tcpSocket.connect(InetSocketAddress(host, port), 8000)
        Log.i(TAG, "[TLS DIAG] Step 1 SUCCESS: TCP connected to $host:$port from local ${tcpSocket.localSocketAddress}")

        // Step 2: Layer SSL on top with explicit host and port (ensures SNI is set properly)
        Log.i(TAG, "[TLS DIAG] Step 2: Layering SSLSocket over TCP...")
        val socket = sslContext.socketFactory.createSocket(
            tcpSocket,
            host,
            port,
            true // autoClose
        ) as SSLSocket

        socket.tcpNoDelay = true
        socket.keepAlive = true
        socket.soTimeout = 0 // Infinite read timeout for streaming

        // Ensure TLS 1.3 is enabled (with TLS 1.2 fallback)
        val supported = socket.supportedProtocols ?: emptyArray()
        val targetProtocols = listOf("TLSv1.3", "TLSv1.2").filter { it in supported }
        if (targetProtocols.isNotEmpty()) {
            socket.enabledProtocols = targetProtocols.toTypedArray()
        }
        Log.i(TAG, "[TLS DIAG] Configured protocols: ${socket.enabledProtocols?.joinToString(", ")}")

        // Disable HTTPS endpoint identification (prevents hostname mismatch rejection on self-signed IP certs)
        val params = socket.sslParameters
        params.endpointIdentificationAlgorithm = null
        socket.sslParameters = params

        // Step 3: Execute TLS handshake
        Log.i(TAG, "[TLS DIAG] Step 3: Starting TLS handshake...")
        try {
            socket.startHandshake()
            Log.i(TAG, "[TLS DIAG] Step 3 SUCCESS! Protocol=${socket.session.protocol}, Cipher=${socket.session.cipherSuite}")
        } catch (e: Exception) {
            Log.e(TAG, "[TLS DIAG] Step 3 FAILED: TLS handshake error: ${e.javaClass.simpleName} - ${e.message}", e)
            try { socket.close() } catch (_: Exception) {}
            try { tcpSocket.close() } catch (_: Exception) {}
            throw e
        }

        val inStream = BufferedInputStream(socket.inputStream, 65536)
        val outStream = BufferedOutputStream(socket.outputStream, 16384)

        synchronized(sendLock) {
            rawSocket = tcpSocket
            sslSocket = socket
            inputStream = inStream
            outputStream = outStream
        }
        isRunning.set(true)
        inputQueue.clear()

        // Dedicated sequential FIFO input sender thread
        senderThread = Thread({
            Log.i(TAG, "LAN Input sender thread started")
            while (isRunning.get()) {
                try {
                    val msg = inputQueue.poll(100, TimeUnit.MILLISECONDS) ?: continue
                    synchronized(sendLock) {
                        outputStream?.let { out ->
                            val seq = sequence.getAndIncrement()
                            writeMessage(out, msg, seq)
                        }
                    }
                } catch (e: InterruptedException) {
                    break
                } catch (e: Exception) {
                    Log.w(TAG, "LAN input write error: ${e.message}")
                }
            }
            Log.i(TAG, "LAN Input sender thread stopped")
        }, "Ward-LanInputSender").apply { start() }
    }

    override fun sendInput(msg: WardMessage) {
        if (!isRunning.get()) return
        inputQueue.offer(msg)
    }

    override fun sendKeyPress(keycode: Int) {
        if (!isRunning.get()) return
        inputQueue.offer(WardMessage.KeyDown(keycode))
        inputQueue.offer(WardMessage.KeyUp(keycode))
    }

    override fun requestKeyframe() {
        sendInput(WardMessage.KeyframeRequest)
    }

    override fun receiveHeader(): Header {
        val stream = inputStream ?: throw IllegalStateException("Transport not connected")
        return readHeader(stream)
    }

    override fun receivePayload(length: Int): ByteArray {
        val stream = inputStream ?: throw IllegalStateException("Transport not connected")
        return readPayload(stream, length)
    }

    override fun disconnect() {
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
                rawSocket?.close()
                sslSocket = null
                rawSocket = null
                outputStream = null
                inputStream = null
            }
        } catch (_: Exception) {}

        Log.i(TAG, "Disconnected LAN TLS transport")
    }
}
