package com.example.ward.transport

import android.util.Log
import com.example.ward.protocol.*
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

private const val TAG = "LanTlsTransport"

/**
 * LAN Transport implementing TLS 1.3 over direct TCP socket with sequential FIFO input queue.
 */
class LanTlsTransport : WardTransport {
    override val transportType: TransportType = TransportType.LAN

    private var sslSocket: SSLSocket? = null
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

        Log.i(TAG, "Connecting LAN TLS 1.3 transport to $host:$port...")

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
        socket.keepAlive = true
        socket.soTimeout = 0 // Infinite read timeout for streaming
        socket.connect(InetSocketAddress(host, port), 8000)
        socket.startHandshake()

        Log.i(TAG, "LAN TLS handshake successful: ${socket.session.protocol} (${socket.session.cipherSuite})")

        val inStream = BufferedInputStream(socket.inputStream, 65536)
        val outStream = BufferedOutputStream(socket.outputStream, 16384)

        synchronized(sendLock) {
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
                sslSocket = null
                outputStream = null
                inputStream = null
            }
        } catch (_: Exception) {}

        Log.i(TAG, "Disconnected LAN TLS transport")
    }
}
