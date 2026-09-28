package com.example.ward.protocol

import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

class WardProtocolTest {

    @Test
    fun testHeaderSerializationRoundtrip() {
        val original = Header(
            magic = PROTOCOL_MAGIC,
            version = PROTOCOL_VERSION,
            type = MessageType.VIDEO_CONFIG,
            payloadLen = 6,
            sequence = 42
        )
        val bytes = original.toByteArray()
        assertEquals(HEADER_SIZE, bytes.size)
        val deserialized = Header.fromByteArray(bytes)

        assertEquals(original.magic, deserialized.magic)
        assertEquals(original.version, deserialized.version)
        assertEquals(original.type, deserialized.type)
        assertEquals(original.payloadLen, deserialized.payloadLen)
        assertEquals(original.sequence, deserialized.sequence)
    }

    @Test
    fun testConnectionInitSerialization() {
        val msg = WardMessage.ConnectionInit(
            protocolVersion = 1,
            clientType = 1,
            deviceId = ByteArray(32) { it.toByte() },
            deviceName = "Pixel 8"
        )
        val serialized = msg.serialize()
        assertNotNull(serialized)
        assertTrue(serialized.size > 36)
    }

    @Test
    fun testVideoConfigParse() {
        val config = WardMessage.VideoConfig(
            width = 1920,
            height = 1080,
            codec = 1,
            fpsTarget = 60
        )
        val bytes = config.serialize()
        assertEquals(6, bytes.size)

        val parsed = WardMessage.VideoConfig.parse(bytes)
        assertEquals(1920, parsed.width)
        assertEquals(1080, parsed.height)
        assertEquals(1.toShort(), parsed.codec)
        assertEquals(60.toShort(), parsed.fpsTarget)
    }

    @Test
    fun testVideoKeyframeRoundtrip() {
        val nalData = byteArrayOf(0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1f)
        val original = WardMessage.VideoKeyframe(
            timestampNs = 1234567890L,
            data = nalData
        )
        val serialized = original.serialize()
        assertEquals(8 + nalData.size, serialized.size)

        val parsed = WardMessage.VideoKeyframe.parse(serialized)
        assertEquals(1234567890L, parsed.timestampNs)
        assertArrayEquals(nalData, parsed.data)
    }

    @Test
    fun testMouseMoveSerialization() {
        val msg = WardMessage.MouseMove(15.5f, -20.25f)
        val bytes = msg.serialize()
        assertEquals(8, bytes.size)
    }

    @Test
    fun testStreamRoundtrip() {
        val out = ByteArrayOutputStream()
        val msg = WardMessage.MouseMove(100f, 200f)
        writeMessage(out, msg, 1)

        val inputStream = ByteArrayInputStream(out.toByteArray())
        val header = readHeader(inputStream)
        val payload = readPayload(inputStream, header.payloadLen)

        assertEquals(MessageType.MOUSE_MOVE, header.type)
        assertEquals(1, header.sequence)
        assertEquals(8, payload.size)
    }
}
