package com.example.ward.stun

import org.junit.Assert.*
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class StunClientTest {

    @Test
    fun testCreateBindingRequest() {
        val tid = ByteArray(12) { it.toByte() }
        val req = StunClient.createBindingRequest(tid)

        assertEquals(20, req.size)

        val buf = ByteBuffer.wrap(req).order(ByteOrder.BIG_ENDIAN)
        assertEquals(0x0001.toShort(), buf.short) // Binding Request
        assertEquals(0.toShort(), buf.short) // Length 0
        assertEquals(STUN_MAGIC_COOKIE, buf.int) // Magic Cookie

        val extractedTid = ByteArray(12)
        buf.get(extractedTid)
        assertArrayEquals(tid, extractedTid)
    }

    @Test
    fun testParseXorMappedIpv4() {
        val tid = ByteArray(12) { (it + 5).toByte() }

        // Build valid STUN response
        val targetPort = 45678
        val targetIpInt = 0xCB0071C3.toInt() // 203.0.113.195

        val xPort = targetPort xor ((STUN_MAGIC_COOKIE ushr 16) and 0xFFFF)
        val xIp = targetIpInt xor STUN_MAGIC_COOKIE

        val buf = ByteBuffer.allocate(20 + 4 + 8).order(ByteOrder.BIG_ENDIAN)
        // Header
        buf.putShort(BINDING_RESPONSE)
        buf.putShort(12.toShort()) // attribute length (4 header + 8 value)
        buf.putInt(STUN_MAGIC_COOKIE)
        buf.put(tid)

        // XOR-MAPPED-ADDRESS attribute
        buf.putShort(ATTR_XOR_MAPPED_ADDRESS)
        buf.putShort(8.toShort())
        buf.put(0x00.toByte()) // Reserved
        buf.put(0x01.toByte()) // IPv4
        buf.putShort(xPort.toShort())
        buf.putInt(xIp)

        val result = StunClient.parseBindingResponse(buf.array(), tid)
        assertNotNull(result)
        assertEquals("203.0.113.195", result!!.first)
        assertEquals(45678, result.second)
    }

    @Test
    fun testParseMappedAddressIpv4() {
        val tid = ByteArray(12) { (it + 1).toByte() }

        val buf = ByteBuffer.allocate(20 + 4 + 8).order(ByteOrder.BIG_ENDIAN)
        // Header
        buf.putShort(BINDING_RESPONSE)
        buf.putShort(12.toShort())
        buf.putInt(STUN_MAGIC_COOKIE)
        buf.put(tid)

        // MAPPED-ADDRESS attribute
        buf.putShort(ATTR_MAPPED_ADDRESS)
        buf.putShort(8.toShort())
        buf.put(0x00.toByte())
        buf.put(0x01.toByte()) // IPv4
        buf.putShort(12345.toShort())
        buf.put(192.toByte())
        buf.put(168.toByte())
        buf.put(1.toByte())
        buf.put(50.toByte())

        val result = StunClient.parseBindingResponse(buf.array(), tid)
        assertNotNull(result)
        assertEquals("192.168.1.50", result!!.first)
        assertEquals(12345, result.second)
    }
}
