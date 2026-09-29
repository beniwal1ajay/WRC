package com.example.ward.network

import android.util.Log
import com.example.ward.transport.TransportType
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.withContext
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket

private const val TAG = "ConnectionSelector"

/**
 * Connection tier for automatic network selection, ordered by performance and cost preference.
 */
enum class ConnectionTier(val preferenceScore: Int, val displayName: String) {
    LAN_DIRECT(1000, "Direct LAN"),
    VPN_DIRECT(800, "VPN Tunnel"),
    P2P_ICE(600, "Direct P2P"),
    TURN_RELAY(200, "TURN Relay");

    fun toTransportType(): TransportType = when (this) {
        LAN_DIRECT -> TransportType.LAN
        VPN_DIRECT -> TransportType.VPN
        P2P_ICE -> TransportType.DIRECT_P2P
        TURN_RELAY -> TransportType.RELAY
    }
}

data class CandidateEndpoint(
    val tier: ConnectionTier,
    val host: String,
    val port: Int,
    val label: String
)

data class ProbeResult(
    val endpoint: CandidateEndpoint,
    val rttMs: Long,
    val success: Boolean
)

/**
 * Classifies an IP address into its probable ConnectionTier.
 */
fun classifyEndpointIp(ip: String, ifaceNameHint: String? = null): ConnectionTier {
    if (ifaceNameHint != null) {
        if (ifaceNameHint.startsWith("tailscale") || ifaceNameHint.startsWith("wg") || ifaceNameHint.startsWith("tun")) {
            return ConnectionTier.VPN_DIRECT
        }
    }

    try {
        val addr = InetAddress.getByName(ip)
        val octets = addr.address

        if (octets.size == 4) {
            val b0 = octets[0].toInt() and 0xFF
            val b1 = octets[1].toInt() and 0xFF

            // Tailscale CGNAT (100.64.0.0/10)
            if (b0 == 100 && (b1 in 64..127)) {
                return ConnectionTier.VPN_DIRECT
            }

            // Private RFC 1918 / Loopback
            if (b0 == 10 || (b0 == 172 && (b1 in 16..31)) || (b0 == 192 && b1 == 168) || b0 == 127) {
                return ConnectionTier.LAN_DIRECT
            }

            return ConnectionTier.P2P_ICE
        } else if (octets.size == 16) {
            if (addr.isLoopbackAddress) {
                return ConnectionTier.LAN_DIRECT
            }
            return ConnectionTier.P2P_ICE
        }
    } catch (_: Exception) {}

    return ConnectionTier.P2P_ICE
}

object ConnectionSelector {

    /**
     * Sorts candidate endpoints by tier preference score (descending).
     */
    fun rankCandidates(candidates: List<CandidateEndpoint>): List<CandidateEndpoint> {
        return candidates.sortedByDescending { it.tier.preferenceScore }
    }

    /**
     * Probe a single TCP endpoint to measure round-trip latency.
     */
    fun probeTcpEndpoint(endpoint: CandidateEndpoint, timeoutMs: Int = 1500): ProbeResult {
        val start = System.currentTimeMillis()
        val socket = Socket()
        return try {
            socket.tcpNoDelay = true
            socket.connect(InetSocketAddress(endpoint.host, endpoint.port), timeoutMs)
            val rtt = System.currentTimeMillis() - start
            Log.d(TAG, "Probe succeeded for ${endpoint.label} (${endpoint.host}:${endpoint.port}): ${rtt}ms")
            ProbeResult(endpoint, rtt, true)
        } catch (e: Exception) {
            Log.d(TAG, "Probe failed for ${endpoint.label} (${endpoint.host}:${endpoint.port}): ${e.message}")
            ProbeResult(endpoint, Long.MAX_VALUE, false)
        } finally {
            try {
                socket.close()
            } catch (_: Exception) {}
        }
    }

    /**
     * Select the optimal connection candidate according to the multi-tier hierarchy:
     * Tier 1 (LAN Direct) -> Tier 2 (VPN Direct) -> Tier 3 (P2P ICE) -> Tier 4 (TURN Relay).
     *
     * Evaluates candidates tier-by-tier with parallel probing within each tier.
     */
    suspend fun selectBestEndpoint(
        candidates: List<CandidateEndpoint>,
        probeTimeoutMs: Int = 1500
    ): ProbeResult? = withContext(Dispatchers.IO) {
        if (candidates.isEmpty()) return@withContext null

        val ranked = rankCandidates(candidates)
        Log.i(TAG, "Evaluating ${ranked.size} candidate endpoints across tiers...")

        // Group by tier maintaining descending priority
        val tierGroups = ArrayList<MutableList<CandidateEndpoint>>()
        for (c in ranked) {
            val lastGroup = tierGroups.lastOrNull()
            if (lastGroup != null && lastGroup[0].tier == c.tier) {
                lastGroup.add(c)
            } else {
                tierGroups.add(mutableListOf(c))
            }
        }

        // Test tier by tier
        for (group in tierGroups) {
            val tierName = group[0].tier.displayName
            Log.d(TAG, "Probing tier '$tierName' with ${group.size} candidate(s)...")

            val deferredProbes = group.map { ep ->
                async { probeTcpEndpoint(ep, probeTimeoutMs) }
            }

            val results = deferredProbes.awaitAll()
            val successful = results.filter { it.success }.sortedBy { it.rttMs }

            if (successful.isNotEmpty()) {
                val winner = successful[0]
                Log.i(TAG, "Selected connection: ${winner.endpoint.tier.displayName} via ${winner.endpoint.host}:${winner.endpoint.port} (${winner.rttMs}ms)")
                return@withContext winner
            }
        }

        Log.w(TAG, "All candidate endpoints failed connectivity probes")
        null
    }
}
