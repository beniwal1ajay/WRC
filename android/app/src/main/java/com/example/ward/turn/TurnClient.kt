package com.example.ward.turn

import android.util.Log
import com.example.ward.stun.STUN_MAGIC_COOKIE
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

private const val TAG = "TurnClient"

// TURN Message Types (RFC 5766 / RFC 8656)
const val ALLOCATE_REQUEST = 0x0003.toShort()
const val ALLOCATE_RESPONSE = 0x0103.toShort()
const val ALLOCATE_ERROR_RESPONSE = 0x0113.toShort()
const val REFRESH_REQUEST = 0x0004.toShort()
const val REFRESH_RESPONSE = 0x0104.toShort()
const val CREATE_PERMISSION_REQUEST = 0x0008.toShort()
const val CREATE_PERMISSION_RESPONSE = 0x0108.toShort()
const val SEND_INDICATION = 0x0016.toShort()
const val DATA_INDICATION = 0x0117.toShort()

// TURN Attribute Types
const val ATTR_USERNAME = 0x0006.toShort()
const val ATTR_MESSAGE_INTEGRITY = 0x0008.toShort()
const val ATTR_ERROR_CODE = 0x0009.toShort()
const val ATTR_LIFETIME = 0x000D.toShort()
const val ATTR_XOR_PEER_ADDRESS = 0x0012.toShort()
const val ATTR_DATA = 0x0013.toShort()
const val ATTR_REALM = 0x0014.toShort()
const val ATTR_NONCE = 0x0015.toShort()
const val ATTR_XOR_RELAYED_ADDRESS = 0x0016.toShort()
const val ATTR_REQUESTED_TRANSPORT = 0x0019.toShort()
const val ATTR_XOR_MAPPED_ADDRESS = 0x0020.toShort()

data class TurnConfig(
    val server: String,
    val username: String,
    val password: String,
    val realm: String? = null
)

data class TurnAllocation(
    val relayedIp: String,
    val relayedPort: Int,
    val mappedIp: String?,
    val mappedPort: Int?,
    val lifetimeSecs: Int,
    val realm: String,
    val nonce: String,
    val key: ByteArray,
    val turnServerHost: String,
    val turnServerPort: Int
)

/**
 * Generate 16-byte key for RFC 5389 / RFC 5766 Long-Term Credential Mechanism:
 * key = MD5(username + ":" + realm + ":" + password)
 */
fun generateLongTermKey(username: String, realm: String, password: String): ByteArray {
    val md5 = MessageDigest.getInstance("MD5")
    return md5.digest("$username:$realm:$password".toByteArray(Charsets.UTF_8))
}

/**
 * Append attribute with 4-byte padding alignment.
 */
fun appendAttribute(buf: ByteBuffer, attrType: Short, value: ByteArray) {
    buf.putShort(attrType)
    buf.putShort(value.size.toShort())
    buf.put(value)
    val pad = (4 - (value.size % 4)) % 4
    for (i in 0 until pad) {
        buf.put(0.toByte())
    }
}

/**
 * Encode an IP and port into a STUN XOR address attribute payload.
 */
fun encodeXorAddress(ip: String, port: Int, tid: ByteArray): ByteArray {
    val addr = InetAddress.getByName(ip)
    val ipBytes = addr.address
    val isV6 = ipBytes.size == 16
    val attrLen = if (isV6) 20 else 8

    val buf = ByteBuffer.allocate(attrLen).order(ByteOrder.BIG_ENDIAN)
    buf.put(0.toByte()) // reserved
    buf.put((if (isV6) 0x02 else 0x01).toByte()) // family

    val xPort = port xor ((STUN_MAGIC_COOKIE ushr 16) and 0xFFFF)
    buf.putShort(xPort.toShort())

    if (isV6) {
        val mask = ByteBuffer.allocate(16).order(ByteOrder.BIG_ENDIAN)
        mask.putInt(STUN_MAGIC_COOKIE)
        mask.put(tid)
        val maskBytes = mask.array()
        for (i in 0 until 16) {
            buf.put((ipBytes[i].toInt() xor maskBytes[i].toInt()).toByte())
        }
    } else {
        val ipInt = ByteBuffer.wrap(ipBytes).int
        val xIp = ipInt xor STUN_MAGIC_COOKIE
        buf.putInt(xIp)
    }

    return buf.array()
}

/**
 * Parse a STUN XOR address attribute payload.
 */
fun parseXorAddress(attrVal: ByteArray, tid: ByteArray): Pair<String, Int>? {
    if (attrVal.size < 8) return null
    val family = attrVal[1].toInt() and 0xFF
    val xPort = ByteBuffer.wrap(attrVal, 2, 2).order(ByteOrder.BIG_ENDIAN).short.toInt() and 0xFFFF
    val port = xPort xor ((STUN_MAGIC_COOKIE ushr 16) and 0xFFFF)

    if (family == 0x01 && attrVal.size >= 8) {
        val xIp = ByteBuffer.wrap(attrVal, 4, 4).order(ByteOrder.BIG_ENDIAN).int
        val ipInt = xIp xor STUN_MAGIC_COOKIE
        val ipBytes = ByteBuffer.allocate(4).putInt(ipInt).array()
        val ip = InetAddress.getByAddress(ipBytes).hostAddress ?: return null
        return Pair(ip, port)
    } else if (family == 0x02 && attrVal.size >= 20) {
        val mask = ByteBuffer.allocate(16).order(ByteOrder.BIG_ENDIAN)
        mask.putInt(STUN_MAGIC_COOKIE)
        mask.put(tid)
        val maskBytes = mask.array()

        val ipBytes = ByteArray(16)
        for (i in 0 until 16) {
            ipBytes[i] = (attrVal[4 + i].toInt() xor maskBytes[i].toInt()).toByte()
        }
        val ip = InetAddress.getByAddress(ipBytes).hostAddress ?: return null
        return Pair(ip, port)
    }
    return null
}

/**
 * Add MESSAGE-INTEGRITY attribute according to RFC 5389 Section 15.4.
 */
fun addMessageIntegrity(msg: ByteArray, key: ByteArray): ByteArray {
    val currentPayload = msg.size - 20
    val totalPayload = currentPayload + 24 // 4 byte header + 20 byte HMAC-SHA1

    val updatedMsg = msg.copyOf(msg.size + 24)
    ByteBuffer.wrap(updatedMsg, 2, 2).order(ByteOrder.BIG_ENDIAN).putShort(totalPayload.toShort())

    val mac = Mac.getInstance("HmacSHA1")
    val secretKey = SecretKeySpec(key, "HmacSHA1")
    mac.init(secretKey)
    mac.update(updatedMsg, 0, msg.size)
    val signature = mac.doFinal()

    val buf = ByteBuffer.wrap(updatedMsg, msg.size, 24).order(ByteOrder.BIG_ENDIAN)
    buf.putShort(ATTR_MESSAGE_INTEGRITY)
    buf.putShort(20.toShort())
    buf.put(signature)

    return updatedMsg
}

/**
 * Verify MESSAGE-INTEGRITY attribute of an incoming STUN message.
 */
fun verifyMessageIntegrity(data: ByteArray, key: ByteArray): Boolean {
    if (data.size < 24) return false

    var offset = 20
    val end = data.size

    while (offset + 4 <= end) {
        val attrType = ByteBuffer.wrap(data, offset, 2).order(ByteOrder.BIG_ENDIAN).short
        val attrLen = ByteBuffer.wrap(data, offset + 2, 2).order(ByteOrder.BIG_ENDIAN).short.toInt() and 0xFFFF
        val attrEnd = offset + 4 + attrLen

        if (attrType == ATTR_MESSAGE_INTEGRITY && attrLen == 20 && attrEnd <= end) {
            val receivedMi = data.copyOfRange(offset + 4, attrEnd)

            val subMsg = data.copyOf(offset)
            val subPayloadLen = offset - 20 + 24
            ByteBuffer.wrap(subMsg, 2, 2).order(ByteOrder.BIG_ENDIAN).putShort(subPayloadLen.toShort())

            val mac = Mac.getInstance("HmacSHA1")
            val secretKey = SecretKeySpec(key, "HmacSHA1")
            mac.init(secretKey)
            mac.update(subMsg)
            val computed = mac.doFinal()

            return computed.contentEquals(receivedMi)
        }

        offset = (attrEnd + 3) and 3.inv()
    }
    return false
}

/**
 * Create a TURN SendIndication packet (RFC 5766 Section 10).
 */
fun createSendIndication(peerIp: String, peerPort: Int, data: ByteArray): ByteArray {
    val random = SecureRandom()
    val tid = ByteArray(12)
    random.nextBytes(tid)

    val peerVal = encodeXorAddress(peerIp, peerPort, tid)
    val peerAttrPad = (4 - (peerVal.size % 4)) % 4
    val dataAttrPad = (4 - (data.size % 4)) % 4

    val totalLen = 20 + (4 + peerVal.size + peerAttrPad) + (4 + data.size + dataAttrPad)
    val buf = ByteBuffer.allocate(totalLen).order(ByteOrder.BIG_ENDIAN)

    // Header
    buf.putShort(SEND_INDICATION)
    buf.putShort((totalLen - 20).toShort())
    buf.putInt(STUN_MAGIC_COOKIE)
    buf.put(tid)

    // Attributes
    appendAttribute(buf, ATTR_XOR_PEER_ADDRESS, peerVal)
    appendAttribute(buf, ATTR_DATA, data)

    return buf.array()
}

/**
 * Parse a TURN DataIndication packet (RFC 5766 Section 10.4).
 */
fun parseDataIndication(data: ByteArray): Pair<Pair<String, Int>, ByteArray>? {
    if (data.size < 20) return null

    val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
    val msgType = buf.short
    if (msgType != DATA_INDICATION) return null

    val msgLen = buf.short.toInt() and 0xFFFF
    val magic = buf.int
    if (magic != STUN_MAGIC_COOKIE) return null

    val tid = ByteArray(12)
    buf.get(tid)

    val limit = minOf(data.size, 20 + msgLen)
    var peerAddr: Pair<String, Int>? = null
    var payload: ByteArray? = null

    while (buf.position() + 4 <= limit) {
        val attrType = buf.short
        val attrLen = buf.short.toInt() and 0xFFFF
        if (buf.remaining() < attrLen) break
        val startPos = buf.position()

        when (attrType) {
            ATTR_XOR_PEER_ADDRESS -> {
                val bytes = ByteArray(attrLen)
                buf.get(bytes)
                peerAddr = parseXorAddress(bytes, tid)
            }
            ATTR_DATA -> {
                val bytes = ByteArray(attrLen)
                buf.get(bytes)
                payload = bytes
            }
            else -> {
                buf.position(startPos + attrLen)
            }
        }

        val paddedLen = (attrLen + 3) and 3.inv()
        buf.position(startPos + paddedLen)
    }

    if (peerAddr != null && payload != null) {
        return Pair(peerAddr, payload)
    }
    return null
}

/**
 * TURN relay client for Android.
 */
class TurnClient(val socket: DatagramSocket) {

    private val random = SecureRandom()

    fun allocate(config: TurnConfig): TurnAllocation? {
        try {
            val parts = config.server.split(":")
            val host = parts[0]
            val port = if (parts.size > 1) parts[1].toInt() else 3478
            val serverAddr = InetAddress.getByName(host)

            // 1. Send unauthenticated Allocate request
            val tid1 = ByteArray(12)
            random.nextBytes(tid1)

            val req1Buf = ByteBuffer.allocate(28).order(ByteOrder.BIG_ENDIAN)
            req1Buf.putShort(ALLOCATE_REQUEST)
            req1Buf.putShort(8.toShort()) // 4 byte header + 4 byte value
            req1Buf.putInt(STUN_MAGIC_COOKIE)
            req1Buf.put(tid1)
            appendAttribute(req1Buf, ATTR_REQUESTED_TRANSPORT, byteArrayOf(17, 0, 0, 0))

            val packet1 = DatagramPacket(req1Buf.array(), req1Buf.array().size, serverAddr, port)
            socket.send(packet1)

            val origTimeout = socket.soTimeout
            socket.soTimeout = 3000
            val recvBuf = ByteArray(2048)

            try {
                val recvPacket = DatagramPacket(recvBuf, recvBuf.size)
                socket.receive(recvPacket)

                val resp1Data = recvPacket.data.copyOf(recvPacket.length)
                if (resp1Data.size < 20) return null

                val respType = ByteBuffer.wrap(resp1Data, 0, 2).order(ByteOrder.BIG_ENDIAN).short
                val (realm, nonce) = if (respType == ALLOCATE_ERROR_RESPONSE) {
                    extractRealmNonce(resp1Data) ?: return null
                } else if (respType == ALLOCATE_RESPONSE) {
                    val info = extractAllocationInfo(resp1Data, tid1) ?: return null
                    return TurnAllocation(
                        relayedIp = info.first.first,
                        relayedPort = info.first.second,
                        mappedIp = info.second?.first,
                        mappedPort = info.second?.second,
                        lifetimeSecs = info.third,
                        realm = "",
                        nonce = "",
                        key = ByteArray(16),
                        turnServerHost = host,
                        turnServerPort = port
                    )
                } else {
                    return null
                }

                // 2. Authenticated Allocate request
                val key = generateLongTermKey(config.username, realm, config.password)
                val tid2 = ByteArray(12)
                random.nextBytes(tid2)

                val userBytes = config.username.toByteArray(Charsets.UTF_8)
                val realmBytes = realm.toByteArray(Charsets.UTF_8)
                val nonceBytes = nonce.toByteArray(Charsets.UTF_8)

                var attrSize = 8 // REQUESTED-TRANSPORT
                attrSize += 4 + 4 // LIFETIME
                attrSize += 4 + userBytes.size + ((4 - (userBytes.size % 4)) % 4)
                attrSize += 4 + realmBytes.size + ((4 - (realmBytes.size % 4)) % 4)
                attrSize += 4 + nonceBytes.size + ((4 - (nonceBytes.size % 4)) % 4)

                val req2Buf = ByteBuffer.allocate(20 + attrSize).order(ByteOrder.BIG_ENDIAN)
                req2Buf.putShort(ALLOCATE_REQUEST)
                req2Buf.putShort(attrSize.toShort())
                req2Buf.putInt(STUN_MAGIC_COOKIE)
                req2Buf.put(tid2)

                appendAttribute(req2Buf, ATTR_REQUESTED_TRANSPORT, byteArrayOf(17, 0, 0, 0))
                val lifetimeVal = ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN).putInt(600).array()
                appendAttribute(req2Buf, ATTR_LIFETIME, lifetimeVal)
                appendAttribute(req2Buf, ATTR_USERNAME, userBytes)
                appendAttribute(req2Buf, ATTR_REALM, realmBytes)
                appendAttribute(req2Buf, ATTR_NONCE, nonceBytes)

                val authenticatedReq = addMessageIntegrity(req2Buf.array(), key)

                val packet2 = DatagramPacket(authenticatedReq, authenticatedReq.size, serverAddr, port)
                socket.send(packet2)

                val recvPacket2 = DatagramPacket(recvBuf, recvBuf.size)
                socket.receive(recvPacket2)

                val resp2Data = recvPacket2.data.copyOf(recvPacket2.length)
                val resp2Type = ByteBuffer.wrap(resp2Data, 0, 2).order(ByteOrder.BIG_ENDIAN).short
                if (resp2Type != ALLOCATE_RESPONSE) return null

                val info = extractAllocationInfo(resp2Data, tid2) ?: return null
                Log.i(TAG, "TURN allocation succeeded: ${info.first.first}:${info.first.second}")

                return TurnAllocation(
                    relayedIp = info.first.first,
                    relayedPort = info.first.second,
                    mappedIp = info.second?.first,
                    mappedPort = info.second?.second,
                    lifetimeSecs = info.third,
                    realm = realm,
                    nonce = nonce,
                    key = key,
                    turnServerHost = host,
                    turnServerPort = port
                )
            } finally {
                socket.soTimeout = origTimeout
            }
        } catch (e: Exception) {
            Log.w(TAG, "TURN allocation failed: ${e.message}")
            return null
        }
    }

    suspend fun allocateAsync(config: TurnConfig): TurnAllocation? = withContext(Dispatchers.IO) {
        allocate(config)
    }

    private fun extractRealmNonce(data: ByteArray): Pair<String, String>? {
        val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
        buf.position(2)
        val msgLen = buf.short.toInt() and 0xFFFF
        buf.position(20)

        val limit = minOf(data.size, 20 + msgLen)
        var realm: String? = null
        var nonce: String? = null

        while (buf.position() + 4 <= limit) {
            val attrType = buf.short
            val attrLen = buf.short.toInt() and 0xFFFF
            if (buf.remaining() < attrLen) break
            val startPos = buf.position()

            if (attrType == ATTR_REALM) {
                val bytes = ByteArray(attrLen)
                buf.get(bytes)
                realm = String(bytes, Charsets.UTF_8)
            } else if (attrType == ATTR_NONCE) {
                val bytes = ByteArray(attrLen)
                buf.get(bytes)
                nonce = String(bytes, Charsets.UTF_8)
            } else {
                buf.position(startPos + attrLen)
            }

            val paddedLen = (attrLen + 3) and 3.inv()
            buf.position(startPos + paddedLen)
        }

        if (realm != null && nonce != null) {
            return Pair(realm, nonce)
        }
        return null
    }

    private fun extractAllocationInfo(
        data: ByteArray,
        tid: ByteArray
    ): Triple<Pair<String, Int>, Pair<String, Int>?, Int>? {
        val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
        buf.position(2)
        val msgLen = buf.short.toInt() and 0xFFFF
        buf.position(20)

        val limit = minOf(data.size, 20 + msgLen)
        var relayedAddr: Pair<String, Int>? = null
        var mappedAddr: Pair<String, Int>? = null
        var lifetimeSecs = 600

        while (buf.position() + 4 <= limit) {
            val attrType = buf.short
            val attrLen = buf.short.toInt() and 0xFFFF
            if (buf.remaining() < attrLen) break
            val startPos = buf.position()

            when (attrType) {
                ATTR_XOR_RELAYED_ADDRESS -> {
                    val bytes = ByteArray(attrLen)
                    buf.get(bytes)
                    relayedAddr = parseXorAddress(bytes, tid)
                }
                ATTR_XOR_MAPPED_ADDRESS -> {
                    val bytes = ByteArray(attrLen)
                    buf.get(bytes)
                    mappedAddr = parseXorAddress(bytes, tid)
                }
                ATTR_LIFETIME -> {
                    if (attrLen >= 4) {
                        lifetimeSecs = buf.int
                    } else {
                        buf.position(startPos + attrLen)
                    }
                }
                else -> {
                    buf.position(startPos + attrLen)
                }
            }

            val paddedLen = (attrLen + 3) and 3.inv()
            buf.position(startPos + paddedLen)
        }

        if (relayedAddr != null) {
            return Triple(relayedAddr, mappedAddr, lifetimeSecs)
        }
        return null
    }
}
