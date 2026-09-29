package com.example.ward.network

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import java.net.ServerSocket

class ConnectionSelectorTest {

    @Test
    fun testConnectionTierPriority() {
        assertTrue(ConnectionTier.LAN_DIRECT.preferenceScore > ConnectionTier.VPN_DIRECT.preferenceScore)
        assertTrue(ConnectionTier.VPN_DIRECT.preferenceScore > ConnectionTier.P2P_ICE.preferenceScore)
        assertTrue(ConnectionTier.P2P_ICE.preferenceScore > ConnectionTier.TURN_RELAY.preferenceScore)
    }

    @Test
    fun testIpClassification() {
        assertEquals(ConnectionTier.LAN_DIRECT, classifyEndpointIp("192.168.1.100"))
        assertEquals(ConnectionTier.LAN_DIRECT, classifyEndpointIp("10.0.1.5"))
        assertEquals(ConnectionTier.LAN_DIRECT, classifyEndpointIp("127.0.0.1"))

        assertEquals(ConnectionTier.VPN_DIRECT, classifyEndpointIp("100.80.20.10")) // Tailscale CGNAT
        assertEquals(ConnectionTier.VPN_DIRECT, classifyEndpointIp("10.100.0.1", "wg0"))

        assertEquals(ConnectionTier.P2P_ICE, classifyEndpointIp("203.0.113.15"))
    }

    @Test
    fun testCandidateRanking() {
        val candidates = listOf(
            CandidateEndpoint(ConnectionTier.TURN_RELAY, "1.1.1.1", 3478, "Relay"),
            CandidateEndpoint(ConnectionTier.LAN_DIRECT, "192.168.1.2", 51820, "LAN"),
            CandidateEndpoint(ConnectionTier.VPN_DIRECT, "100.64.0.1", 51820, "VPN")
        )

        val ranked = ConnectionSelector.rankCandidates(candidates)
        assertEquals(ConnectionTier.LAN_DIRECT, ranked[0].tier)
        assertEquals(ConnectionTier.VPN_DIRECT, ranked[1].tier)
        assertEquals(ConnectionTier.TURN_RELAY, ranked[2].tier)
    }

    @Test
    fun testSelectBestEndpointPrefersWorkingLanOverRelay() = runBlocking {
        val server = ServerSocket(0)
        val port = server.localPort

        try {
            val candidates = listOf(
                CandidateEndpoint(ConnectionTier.TURN_RELAY, "127.0.0.1", port, "Relay"),
                CandidateEndpoint(ConnectionTier.LAN_DIRECT, "127.0.0.1", port, "LAN")
            )

            val winner = ConnectionSelector.selectBestEndpoint(candidates, 1000)
            assertNotNull(winner)
            assertEquals(ConnectionTier.LAN_DIRECT, winner!!.endpoint.tier)
            assertTrue(winner.success)
        } finally {
            server.close()
        }
    }

    @Test
    fun testSelectBestEndpointFallbackWhenLanFails() = runBlocking {
        val server = ServerSocket(0)
        val port = server.localPort

        try {
            val candidates = listOf(
                CandidateEndpoint(ConnectionTier.LAN_DIRECT, "127.0.0.1", 1, "Dead LAN"), // port 1 fails
                CandidateEndpoint(ConnectionTier.TURN_RELAY, "127.0.0.1", port, "Working Relay")
            )

            val winner = ConnectionSelector.selectBestEndpoint(candidates, 500)
            assertNotNull(winner)
            assertEquals(ConnectionTier.TURN_RELAY, winner!!.endpoint.tier)
            assertEquals(port, winner.endpoint.port)
        } finally {
            server.close()
        }
    }
}
