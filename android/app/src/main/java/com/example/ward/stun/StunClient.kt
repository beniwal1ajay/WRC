package com.example.ward.stun

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.SecureRandom

private const val TAG = "StunClient"

const val STUN_MAGIC_COOKIE = 0x2112A442
const val BINDING_REQUEST = 0x0001.toShort()
const val BINDING_RESPONSE = 0x0101.toShort()
const val ATTR_MAPPED_ADDRESS = 0x0001.toShort()
const val ATTR_XOR_MAPPED_ADDRESS = 0x0020.toShort()

val DEFAULT_STUN_SERVERS = listOf(
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun2.l.google.com:19302"
)

data class StunEndpoint(
    val publicIp: String,
    val publicPort: Int,
    val localPort: Int,
    val stunServer: String
)

/**
 * RFC 5389 compliant STUN client for public reflexive IP and port resolution.
 */
object StunClient {

    fun createBindingRequest(transactionId: ByteArray): ByteArray {
        val buf = ByteBuffer.allocate(20).order(ByteOrder.BIG_ENDIAN)
        buf.putShort(BINDING_REQUEST)
        buf.putShort(0.toShort()) // length = 0
        buf.putInt(STUN_MAGIC_COOKIE)
        buf.put(transactionId)
        return buf.array()
    }

    fun parseBindingResponse(data: ByteArray, expectedTid: ByteArray): Pair<String, Int>? {
        if (data.size < 20) return null

        val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
        val msgType = buf.short
        if (msgType != BINDING_RESPONSE) return null

        val msgLen = buf.short.toInt() and 0xFFFF
        val magicCookie = buf.int
        if (magicCookie != STUN_MAGIC_COOKIE) return null

        val tid = ByteArray(12)
        buf.get(tid)
        if (!tid.contentEquals(expectedTid)) return null

        val limit = minOf(data.size, 20 + msgLen)
        var mappedResult: Pair<String, Int>? = null

        while (buf.position() + 4 <= limit) {
            val attrType = buf.short
            val attrLen = buf.short.toInt() and 0xFFFF

            if (buf.remaining() < attrLen) break
            val attrStart = buf.position()

            if (attrType == ATTR_XOR_MAPPED_ADDRESS && attrLen >= 8) {
                buf.get() // reserved 0x00
                val family = buf.get().toInt() and 0xFF
                val xPort = buf.short.toInt() and 0xFFFF
                val port = xPort xor ((STUN_MAGIC_COOKIE ushr 16) and 0xFFFF)

                if (family == 0x01 && attrLen >= 8) {
                    val xIp = buf.int
                    val ipInt = xIp xor STUN_MAGIC_COOKIE
                    val ipBytes = ByteBuffer.allocate(4).putInt(ipInt).array()
                    val ip = InetAddress.getByAddress(ipBytes).hostAddress ?: "0.0.0.0"
                    return Pair(ip, port)
                }
            } else if (attrType == ATTR_MAPPED_ADDRESS && attrLen >= 8 && mappedResult == null) {
                buf.get() // reserved
                val family = buf.get().toInt() and 0xFF
                val port = buf.short.toInt() and 0xFFFF
                if (family == 0x01 && attrLen >= 8) {
                    val ipBytes = ByteArray(4)
                    buf.get(ipBytes)
                    val ip = InetAddress.getByAddress(ipBytes).hostAddress ?: "0.0.0.0"
                    mappedResult = Pair(ip, port)
                }
            }

            // Seek to next 4-byte boundary
            val paddedLen = (attrLen + 3) and 3.inv()
            buf.position(attrStart + paddedLen)
        }

        return mappedResult
    }

    fun resolvePublicEndpoint(
        servers: List<String> = DEFAULT_STUN_SERVERS,
        timeoutMs: Int = 2500
    ): StunEndpoint? {
        val random = SecureRandom()
        val socket = DatagramSocket()
        socket.soTimeout = timeoutMs
        val localPort = socket.localPort

        try {
            for (server in servers) {
                try {
                    val parts = server.split(":")
                    val host = parts[0]
                    val port = if (parts.size > 1) parts[1].toInt() else 19302
                    val addr = InetAddress.getByName(host)

                    val tid = ByteArray(12)
                    random.nextBytes(tid)
                    val reqBytes = createBindingRequest(tid)

                    val sendPacket = DatagramPacket(reqBytes, reqBytes.size, addr, port)
                    socket.send(sendPacket)

                    val recvBuf = ByteArray(1024)
                    val recvPacket = DatagramPacket(recvBuf, recvBuf.size)
                    socket.receive(recvPacket)

                    val parsed = parseBindingResponse(recvPacket.data.copyOf(recvPacket.length), tid)
                    if (parsed != null) {
                        Log.i(TAG, "STUN resolution successful via $server: ${parsed.first}:${parsed.second}")
                        return StunEndpoint(
                            publicIp = parsed.first,
                            publicPort = parsed.second,
                            localPort = localPort,
                            stunServer = server
                        )
                    }
                } catch (e: Exception) {
                    Log.d(TAG, "STUN query failed for $server: ${e.message}")
                }
            }
        } finally {
            socket.close()
        }

        return null
    }

    suspend fun resolvePublicEndpointAsync(
        servers: List<String> = DEFAULT_STUN_SERVERS
    ): StunEndpoint? = withContext(Dispatchers.IO) {
        resolvePublicEndpoint(servers)
    }
}
