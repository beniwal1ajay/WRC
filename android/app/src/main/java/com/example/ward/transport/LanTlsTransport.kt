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
class LanTlsTransport(
    private val pinnedCertProvider: (() -> String?)? = null,
    private val onCertDiscovered: ((String) -> Unit)? = null
) : WardTransport {
    override var transportType: TransportType = TransportType.LAN
        private set

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

        transportType = classifyTransportType(host)
        Log.i(TAG, "[TLS DIAG] Starting $transportType connection to $host:$port...")

        // Robust ExtendedTrustManager with TOFU (Trust-On-First-Use) SHA-256 certificate pinning
        val trustAllCerts = arrayOf<TrustManager>(object : X509ExtendedTrustManager() {
            override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {
                val cert = chain?.firstOrNull() ?: throw java.security.cert.CertificateException("Empty certificate chain")
                val sans = try { cert.subjectAlternativeNames?.map { it[1].toString() } } catch (_: Exception) { null }
                val md = java.security.MessageDigest.getInstance("SHA-256")
                val digest = md.digest(cert.encoded)
                val fingerprint = digest.joinToString("") { "%02x".format(it) }

                val pinned = pinnedCertProvider?.invoke()
                if (!pinned.isNullOrBlank()) {
                    if (!fingerprint.equals(pinned.trim(), ignoreCase = true)) {
                        Log.e(TAG, "[SECURITY ALERT] TLS Certificate fingerprint mismatch! Pinned: $pinned, Presented: $fingerprint")
                        throw java.security.cert.CertificateException(
                            "WARD Host Certificate mismatch! Pinned: $pinned, Presented: $fingerprint. Potential MITM attack."
                        )
                    }
                    Log.i(TAG, "[SECURITY] Verified pinned host TLS certificate ($fingerprint)")
                } else {
                    Log.i(TAG, "[SECURITY] Trust-On-First-Use: Discovered host TLS certificate ($fingerprint)")
                    onCertDiscovered?.invoke(fingerprint)
                }
                Log.i(TAG, "[DIAG] Certificate validation: chain_len=${chain.size}, authType=$authType, subject=${cert.subjectDN}, SANs=$sans, result=ACCEPT")
            }
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?, socket: Socket?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?, socket: Socket?) {
                checkServerTrusted(chain, authType)
            }
            override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?, engine: SSLEngine?) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?, engine: SSLEngine?) {
                checkServerTrusted(chain, authType)
            }
        })

        // Use "TLS" as standard context protocol (supports TLS 1.2 and 1.3 across all Android versions)
        val sslContext = try {
            SSLContext.getInstance("TLS").apply {
                init(null, trustAllCerts, SecureRandom())
            }
        } catch (e: Exception) {
            Log.w(TAG, "[DIAG] SSLContext('TLS') init failed, falling back to default: ${e.message}")
            SSLContext.getInstance("Default").apply {
                init(null, trustAllCerts, SecureRandom())
            }
        }

        // Step 1: Connect raw TCP socket first with explicit timeout
        Log.i(TAG, "[ANDROID] Connecting raw TCP socket to $host:$port...")
        val tcpSocket = Socket()
        tcpSocket.tcpNoDelay = true
        tcpSocket.keepAlive = true
        tcpSocket.connect(InetSocketAddress(host, port), 8000)
        Log.i(TAG, "[ANDROID] TCP CONNECTED to $host:$port (local: ${tcpSocket.localSocketAddress})")

        // Step 2: Layer SSL on top with explicit host and port (ensures SNI is set properly)
        val socket = sslContext.socketFactory.createSocket(
            tcpSocket,
            host,
            port,
            true // autoClose
        ) as SSLSocket

        socket.tcpNoDelay = true
        socket.keepAlive = true
        // Set explicit 10s timeout during handshake
        socket.soTimeout = 10000

        // Ensure TLS 1.3 is enabled (with TLS 1.2 fallback)
        val supported = socket.supportedProtocols ?: emptyArray()
        val targetProtocols = listOf("TLSv1.3", "TLSv1.2").filter { it in supported }
        if (targetProtocols.isNotEmpty()) {
            socket.enabledProtocols = targetProtocols.toTypedArray()
        }

        // Disable HTTPS endpoint identification (prevents hostname mismatch rejection on self-signed IP certs)
        val params = socket.sslParameters
        params.endpointIdentificationAlgorithm = null
        socket.sslParameters = params

        // Step 3: Execute TLS handshake
        Log.i(TAG, "[ANDROID] TLS HANDSHAKE BEGIN with $host:$port (protocols=${socket.enabledProtocols?.joinToString(",")})")
        val handshakeStart = System.currentTimeMillis()
        try {
            socket.startHandshake()
            val elapsed = System.currentTimeMillis() - handshakeStart
            Log.i(TAG, "[ANDROID] TLS HANDSHAKE SUCCESS with $host:$port (protocol=${socket.session.protocol}, cipher=${socket.session.cipherSuite}, elapsed=${elapsed}ms)")
            // Restore infinite read timeout for continuous streaming
            socket.soTimeout = 0
        } catch (e: Exception) {
            val elapsed = System.currentTimeMillis() - handshakeStart
            Log.e(TAG, "[ANDROID] TLS HANDSHAKE FAILURE with $host:$port after ${elapsed}ms: ${e.javaClass.name}: ${e.message}", e)
            Log.e(TAG, "[ANDROID] TLS EXCEPTION: ${e.javaClass.name}: ${e.message}")
            try { socket.close() } catch (_: Exception) {}
            try { tcpSocket.close() } catch (_: Exception) {}
            Log.i(TAG, "[ANDROID] SOCKET CLOSED for $host:$port (reason: TLS handshake failure)")
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

        Log.i(TAG, "[ANDROID] SOCKET CLOSED (reason: disconnect requested)")
    }
}
