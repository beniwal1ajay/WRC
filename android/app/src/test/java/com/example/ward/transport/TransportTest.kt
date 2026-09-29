package com.example.ward.transport

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
}
