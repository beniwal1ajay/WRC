package com.example.ward.transport

import com.example.ward.protocol.WardMessage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class TransportTest {

    @Test
    fun testTransportTypeDisplayNames() {
        assertEquals("LAN", TransportType.LAN.displayName)
        assertEquals("VPN", TransportType.VPN.displayName)
        assertEquals("Direct P2P", TransportType.DIRECT_P2P.displayName)
        assertEquals("Relay", TransportType.RELAY.displayName)
    }

    @Test
    fun testLanTlsTransportInstantiation() {
        val transport = LanTlsTransport()
        assertNotNull(transport)
        assertEquals(TransportType.LAN, transport.transportType)
        assertEquals(false, transport.isConnected)
    }

    @Test
    fun testClassifyTransportType() {
        assertEquals(TransportType.VPN, classifyTransportType("100.85.20.45"))
        assertEquals(TransportType.VPN, classifyTransportType("100.115.92.1"))
        assertEquals(TransportType.VPN, classifyTransportType("my-host.ts.net"))
        assertEquals(TransportType.VPN, classifyTransportType("laptop.tailscale"))
        assertEquals(TransportType.LAN, classifyTransportType("192.168.1.100"))
        assertEquals(TransportType.LAN, classifyTransportType("10.31.1.234"))
        assertEquals(TransportType.LAN, classifyTransportType("127.0.0.1"))
    }

    @Test
    fun testMaskIpAddress() {
        assertEquals("100.85.***.***:9740", maskIpAddress("100.85.20.45", 9740))
        assertEquals("192.168.***.***:9740", maskIpAddress("192.168.31.129", 9740))
    }
}
