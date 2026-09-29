package com.example.ward.protocol

import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

const val PROTOCOL_MAGIC: Byte = 0x57 // 'W'
const val PROTOCOL_VERSION: Byte = 0x01
const val HEADER_SIZE = 12

enum class MessageType(val code: Short) {
    CONNECTION_INIT(0x0001),
    AUTH_REQUEST(0x0002),
    AUTH_RESPONSE(0x0003),
    SESSION_ACCEPTED(0x0004),
    SESSION_REJECTED(0x0005),
    VIDEO_FRAME(0x0010),
    VIDEO_KEYFRAME(0x0011),
    VIDEO_CONFIG(0x0012),
    KEYFRAME_REQUEST(0x0013),
    MOUSE_MOVE(0x0020),
    MOUSE_BUTTON(0x0021),
    MOUSE_SCROLL(0x0022),
    KEY_DOWN(0x0030),
    KEY_UP(0x0031),
    CLIPBOARD_SET(0x0040),
    CLIPBOARD_GET(0x0041),
    CLIPBOARD_CONTENT(0x0042),
    PING(0x0050),
    PONG(0x0051),
    STATS(0x0060),
    CANDIDATE_EXCHANGE(0x0070),
    DISCONNECT(0x00F0.toShort()),
    ERROR(0x00FF.toShort());

    companion object {
        private val map = entries.associateBy { it.code }
        fun fromCode(code: Short): MessageType? = map[code]
    }
}

data class Header(
    val magic: Byte = PROTOCOL_MAGIC,
    val version: Byte = PROTOCOL_VERSION,
    val type: MessageType,
    val payloadLen: Int,
    val sequence: Int
) {
    fun toByteArray(): ByteArray {
        val buf = ByteBuffer.allocate(HEADER_SIZE).order(ByteOrder.BIG_ENDIAN)
        buf.put(magic)
        buf.put(version)
        buf.putShort(type.code)
        buf.putInt(payloadLen)
        buf.putInt(sequence)
        return buf.array()
    }

    companion object {
        fun fromByteArray(bytes: ByteArray): Header {
            require(bytes.size >= HEADER_SIZE) { "Header buffer too small" }
            val buf = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN)
            val magic = buf.get()
            require(magic == PROTOCOL_MAGIC) { "Invalid protocol magic: $magic" }
            val version = buf.get()
            val typeCode = buf.short
            val type = MessageType.fromCode(typeCode) ?: throw IllegalArgumentException("Unknown msg type: $typeCode")
            val payloadLen = buf.int
            val sequence = buf.int
            return Header(magic, version, type, payloadLen, sequence)
        }
    }
}

sealed class WardMessage(val type: MessageType) {
    abstract fun serialize(): ByteArray

    data class ConnectionInit(
        val protocolVersion: Short = 1,
        val clientType: Short = 1, // Android
        val deviceId: ByteArray = ByteArray(32),
        val deviceName: String = "Android Device"
    ) : WardMessage(MessageType.CONNECTION_INIT) {
        override fun serialize(): ByteArray {
            val nameBytes = deviceName.toByteArray(Charsets.UTF_8)
            val buf = ByteBuffer.allocate(2 + 2 + 32 + 2 + nameBytes.size).order(ByteOrder.BIG_ENDIAN)
            buf.putShort(protocolVersion)
            buf.putShort(clientType)
            buf.put(deviceId)
            buf.putShort(nameBytes.size.toShort())
            buf.put(nameBytes)
            return buf.array()
        }
    }

    data class AuthRequest(
        val authMethod: Short,
        val challenge: ByteArray
    ) : WardMessage(MessageType.AUTH_REQUEST) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(2 + 2 + challenge.size).order(ByteOrder.BIG_ENDIAN)
            buf.putShort(authMethod)
            buf.putShort(challenge.size.toShort())
            buf.put(challenge)
            return buf.array()
        }

        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is AuthRequest) return false
            if (authMethod != other.authMethod) return false
            return challenge.contentEquals(other.challenge)
        }

        override fun hashCode(): Int {
            var result = authMethod.toInt()
            result = 31 * result + challenge.contentHashCode()
            return result
        }

        companion object {
            fun parse(data: ByteArray): AuthRequest {
                val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
                val method = buf.short
                val len = buf.short.toInt() and 0xFFFF
                val challenge = ByteArray(minOf(len, buf.remaining()))
                buf.get(challenge)
                return AuthRequest(method, challenge)
            }
        }
    }

    data class AuthResponse(
        val authMethod: Short,
        val response: ByteArray
    ) : WardMessage(MessageType.AUTH_RESPONSE) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(2 + 2 + response.size).order(ByteOrder.BIG_ENDIAN)
            buf.putShort(authMethod)
            buf.putShort(response.size.toShort())
            buf.put(response)
            return buf.array()
        }

        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is AuthResponse) return false
            if (authMethod != other.authMethod) return false
            return response.contentEquals(other.response)
        }

        override fun hashCode(): Int {
            var result = authMethod.toInt()
            result = 31 * result + response.contentHashCode()
            return result
        }

        companion object {
            fun parse(data: ByteArray): AuthResponse {
                val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
                val method = buf.short
                val len = buf.short.toInt() and 0xFFFF
                val resp = ByteArray(minOf(len, buf.remaining()))
                buf.get(resp)
                return AuthResponse(method, resp)
            }
        }
    }

    data class SessionAccepted(
        val sessionId: Int,
        val serverDeviceId: ByteArray = ByteArray(32),
        val serverDeviceName: String = "Ubuntu Host"
    ) : WardMessage(MessageType.SESSION_ACCEPTED) {
        override fun serialize(): ByteArray {
            val nameBytes = serverDeviceName.toByteArray(Charsets.UTF_8)
            val buf = ByteBuffer.allocate(4 + 32 + 2 + nameBytes.size).order(ByteOrder.BIG_ENDIAN)
            buf.putInt(sessionId)
            buf.put(serverDeviceId)
            buf.putShort(nameBytes.size.toShort())
            buf.put(nameBytes)
            return buf.array()
        }

        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is SessionAccepted) return false
            if (sessionId != other.sessionId) return false
            if (!serverDeviceId.contentEquals(other.serverDeviceId)) return false
            if (serverDeviceName != other.serverDeviceName) return false
            return true
        }

        override fun hashCode(): Int {
            var result = sessionId
            result = 31 * result + serverDeviceId.contentHashCode()
            result = 31 * result + serverDeviceName.hashCode()
            return result
        }

        companion object {
            fun parse(data: ByteArray): SessionAccepted {
                val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
                val sessionId = buf.int
                if (data.size >= 36) {
                    val deviceId = ByteArray(32)
                    buf.get(deviceId)
                    val name = if (data.size >= 38) {
                        val nameLen = buf.short.toInt() and 0xFFFF
                        val actualLen = minOf(nameLen, buf.remaining())
                        val nameBytes = ByteArray(actualLen)
                        buf.get(nameBytes)
                        String(nameBytes, Charsets.UTF_8)
                    } else {
                        "Ubuntu Host"
                    }
                    return SessionAccepted(sessionId, deviceId, name)
                }
                return SessionAccepted(sessionId)
            }
        }
    }

    data class SessionRejected(
        val reasonCode: Int,
        val reasonMessage: String
    ) : WardMessage(MessageType.SESSION_REJECTED) {
        override fun serialize(): ByteArray {
            val msgBytes = reasonMessage.toByteArray(Charsets.UTF_8)
            val buf = ByteBuffer.allocate(4 + 2 + msgBytes.size).order(ByteOrder.BIG_ENDIAN)
            buf.putInt(reasonCode)
            buf.putShort(msgBytes.size.toShort())
            buf.put(msgBytes)
            return buf.array()
        }

        companion object {
            fun parse(data: ByteArray): SessionRejected {
                val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
                val code = buf.int
                val msg = if (buf.remaining() >= 2) {
                    val len = buf.short.toInt() and 0xFFFF
                    val b = ByteArray(minOf(len, buf.remaining()))
                    buf.get(b)
                    String(b, Charsets.UTF_8)
                } else {
                    ""
                }
                return SessionRejected(code, msg)
            }
        }
    }

    data class VideoConfig(
        val width: Int,
        val height: Int,
        val codec: Short,
        val fpsTarget: Short
    ) : WardMessage(MessageType.VIDEO_CONFIG) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(6).order(ByteOrder.BIG_ENDIAN)
            buf.putShort(width.toShort())
            buf.putShort(height.toShort())
            buf.put(codec.toByte())
            buf.put(fpsTarget.toByte())
            return buf.array()
        }
        companion object {
            fun parse(data: ByteArray): VideoConfig {
                val buf = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
                val width = buf.short.toInt() and 0xFFFF
                val height = buf.short.toInt() and 0xFFFF
                val codec = (buf.get().toInt() and 0xFF).toShort()
                val fps = (buf.get().toInt() and 0xFF).toShort()
                return VideoConfig(width, height, codec, fps)
            }
        }
    }

    data class VideoFrame(
        val timestampNs: Long,
        val data: ByteArray
    ) : WardMessage(MessageType.VIDEO_FRAME) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(8 + data.size).order(ByteOrder.BIG_ENDIAN)
            buf.putLong(timestampNs)
            buf.put(data)
            return buf.array()
        }
        companion object {
            fun parse(payload: ByteArray): VideoFrame {
                val buf = ByteBuffer.wrap(payload).order(ByteOrder.BIG_ENDIAN)
                val ts = buf.long
                val videoData = ByteArray(payload.size - 8)
                buf.get(videoData)
                return VideoFrame(ts, videoData)
            }
        }
    }

    data class VideoKeyframe(
        val timestampNs: Long,
        val data: ByteArray
    ) : WardMessage(MessageType.VIDEO_KEYFRAME) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(8 + data.size).order(ByteOrder.BIG_ENDIAN)
            buf.putLong(timestampNs)
            buf.put(data)
            return buf.array()
        }
        companion object {
            fun parse(payload: ByteArray): VideoKeyframe {
                val buf = ByteBuffer.wrap(payload).order(ByteOrder.BIG_ENDIAN)
                val ts = buf.long
                val videoData = ByteArray(payload.size - 8)
                buf.get(videoData)
                return VideoKeyframe(ts, videoData)
            }
        }
    }

    data class MouseMove(val x: Float, val y: Float) : WardMessage(MessageType.MOUSE_MOVE) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(8).order(ByteOrder.BIG_ENDIAN)
            buf.putFloat(x)
            buf.putFloat(y)
            return buf.array()
        }
    }

    data class MouseButton(val button: Byte, val state: Byte) : WardMessage(MessageType.MOUSE_BUTTON) {
        override fun serialize(): ByteArray = byteArrayOf(button, state)
    }

    data class MouseScroll(val dx: Float, val dy: Float) : WardMessage(MessageType.MOUSE_SCROLL) {
        override fun serialize(): ByteArray {
            val buf = ByteBuffer.allocate(8).order(ByteOrder.BIG_ENDIAN)
            buf.putFloat(dx)
            buf.putFloat(dy)
            return buf.array()
        }
    }

    data class KeyDown(val keycode: Int) : WardMessage(MessageType.KEY_DOWN) {
        override fun serialize(): ByteArray {
            return ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN).putInt(keycode).array()
        }
    }

    data class KeyUp(val keycode: Int) : WardMessage(MessageType.KEY_UP) {
        override fun serialize(): ByteArray {
            return ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN).putInt(keycode).array()
        }
    }

    object KeyframeRequest : WardMessage(MessageType.KEYFRAME_REQUEST) {
        override fun serialize(): ByteArray = ByteArray(0)
    }

    data class CandidateExchange(
        val candidates: List<IceCandidate>
    ) : WardMessage(MessageType.CANDIDATE_EXCHANGE) {
        override fun serialize(): ByteArray {
            var size = 2
            for (c in candidates) {
                val ipBytes = c.ip.toByteArray(Charsets.UTF_8)
                size += 1 + 1 + 2 + ipBytes.size + 2 + 4
            }
            val buf = ByteBuffer.allocate(size).order(ByteOrder.BIG_ENDIAN)
            buf.putShort(candidates.size.toShort())
            for (c in candidates) {
                buf.put(c.candidateType)
                buf.put(c.protocol)
                val ipBytes = c.ip.toByteArray(Charsets.UTF_8)
                buf.putShort(ipBytes.size.toShort())
                buf.put(ipBytes)
                buf.putShort(c.port.toShort())
                buf.putInt(c.priority.toInt())
            }
            return buf.array()
        }

        companion object {
            fun parse(payload: ByteArray): CandidateExchange {
                val buf = ByteBuffer.wrap(payload).order(ByteOrder.BIG_ENDIAN)
                val count = buf.short.toInt() and 0xFFFF
                val list = ArrayList<IceCandidate>(count)
                for (i in 0 until count) {
                    val candidateType = buf.get()
                    val protocol = buf.get()
                    val ipLen = buf.short.toInt() and 0xFFFF
                    val ipBytes = ByteArray(ipLen)
                    buf.get(ipBytes)
                    val ip = String(ipBytes, Charsets.UTF_8)
                    val port = buf.short.toInt() and 0xFFFF
                    val priority = buf.int.toLong() and 0xFFFFFFFFL
                    list.add(IceCandidate(candidateType, protocol, ip, port, priority))
                }
                return CandidateExchange(list)
            }
        }
    }

    object Disconnect : WardMessage(MessageType.DISCONNECT) {
        override fun serialize(): ByteArray = ByteArray(0)
    }
}

data class IceCandidate(
    val candidateType: Byte, // 1 = Host, 2 = Srflx, 3 = Relay
    val protocol: Byte,      // 1 = UDP, 2 = TCP
    val ip: String,
    val port: Int,
    val priority: Long
)

fun writeMessage(out: OutputStream, msg: WardMessage, sequence: Int) {
    val payload = msg.serialize()
    val header = Header(
        type = msg.type,
        payloadLen = payload.size,
        sequence = sequence
    )
    out.write(header.toByteArray())
    if (payload.isNotEmpty()) {
        out.write(payload)
    }
    out.flush()
}

fun readHeader(input: InputStream): Header {
    val buf = ByteArray(HEADER_SIZE)
    var read = 0
    while (read < HEADER_SIZE) {
        val count = input.read(buf, read, HEADER_SIZE - read)
        if (count < 0) throw java.io.EOFException("Connection closed while reading header")
        read += count
    }
    return Header.fromByteArray(buf)
}

fun readPayload(input: InputStream, len: Int): ByteArray {
    val buf = ByteArray(len)
    var read = 0
    while (read < len) {
        val count = input.read(buf, read, len - read)
        if (count < 0) throw java.io.EOFException("Connection closed while reading payload")
        read += count
    }
    return buf
}
