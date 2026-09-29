package com.example.ward.turn

import org.junit.Assert.*
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class TurnClientTest {

    @Test
    fun testLongTermKeyGeneration() {
        val key = generateLongTermKey("alice", "ward.relay", "secretPass123")
        assertEquals(16, key.size)

        val key2 = generateLongTermKey("alice", "ward.relay", "secretPass123")
        assertArrayEquals(key, key2)

        val diffKey = generateLongTermKey("alice", "ward.relay", "wrongPassword")
        assertFalse(key.contentEquals(diffKey))
    }

    @Test
    fun testXorAddressEncodingAndParsing() {
        val tid = ByteArray(12) { (it * 7).toByte() }

        // IPv4
        val v4Ip = "198.51.100.80"
        val v4Port = 51234
        val v4Encoded = encodeXorAddress(v4Ip, v4Port, tid)
        val v4Parsed = parseXorAddress(v4Encoded, tid)
        assertNotNull(v4Parsed)
        assertEquals(v4Ip, v4Parsed!!.first)
        assertEquals(v4Port, v4Parsed.second)

        // IPv6
        val v6Ip = "2001:db8:85a3:0:0:8a2e:370:7334"
        val v6Port = 49152
        val v6Encoded = encodeXorAddress(v6Ip, v6Port, tid)
        val v6Parsed = parseXorAddress(v6Encoded, tid)
        assertNotNull(v6Parsed)
        assertEquals(v6Port, v6Parsed!!.second)
    }

    @Test
    fun testMessageIntegritySigningAndVerification() {
        val key = generateLongTermKey("clientUser", "turn.example.com", "passKey456")

        val buf = ByteBuffer.allocate(64).order(ByteOrder.BIG_ENDIAN)
        buf.putShort(ALLOCATE_REQUEST)
        buf.putShort(0.toShort()) // length placeholder
        buf.putInt(com.example.ward.stun.STUN_MAGIC_COOKIE)
        buf.put(ByteArray(12) { 0x33.toByte() })

        val usernameBytes = "clientUser".toByteArray(Charsets.UTF_8)
        appendAttribute(buf, ATTR_USERNAME, usernameBytes)

        val rawMsg = buf.array().copyOf(buf.position())
        val signedMsg = addMessageIntegrity(rawMsg, key)

        val valid = verifyMessageIntegrity(signedMsg, key)
        assertTrue("Message integrity verification must succeed with valid key", valid)

        val wrongKey = generateLongTermKey("clientUser", "turn.example.com", "wrongPass")
        val invalid = verifyMessageIntegrity(signedMsg, wrongKey)
        assertFalse("Message integrity verification must fail with wrong key", invalid)
    }

    @Test
    fun testSendAndDataIndicationRoundtrip() {
        val peerIp = "203.0.113.77"
        val peerPort = 55123
        val payload = "Hello WARD P2P/TURN Relay!".toByteArray(Charsets.UTF_8)

        val sendPkt = createSendIndication(peerIp, peerPort, payload)
        assertNotNull(sendPkt)

        // Simulate TURN server transforming SendIndication into DataIndication
        val dataPkt = sendPkt.copyOf()
        ByteBuffer.wrap(dataPkt, 0, 2).order(ByteOrder.BIG_ENDIAN).putShort(DATA_INDICATION)

        val parsed = parseDataIndication(dataPkt)
        assertNotNull(parsed)
        assertEquals(peerIp, parsed!!.first.first)
        assertEquals(peerPort, parsed.first.second)
        assertArrayEquals(payload, parsed.second)
    }
}
