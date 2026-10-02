package com.example.ward.transport

/**
 * Supported WARD network transports.
 */
enum class TransportType(val displayName: String) {
    LAN("LAN"),
    VPN("VPN"),
    DIRECT_P2P("Direct P2P"),
    RELAY("Relay")
}

/**
 * Classifies the active network transport type based on host IP or hostname.
 */
fun classifyTransportType(host: String): TransportType {
    val cleanHost = host.trim().lowercase()
    if (cleanHost.startsWith("100.")) {
        val parts = cleanHost.split(".")
        if (parts.size >= 2) {
            val second = parts[1].toIntOrNull() ?: 0
            if (second in 64..127) {
                return TransportType.VPN
            }
        }
        return TransportType.VPN
    }
    if (cleanHost.contains(".ts.net") || cleanHost.contains("tailscale") || cleanHost.contains("wireguard") || cleanHost.contains("vpn")) {
        return TransportType.VPN
    }
    return TransportType.LAN
}

/**
 * Masks IP address to protect privacy in UI diagnostics (e.g. 100.85.***.***:9740).
 */
fun maskIpAddress(host: String, port: Int): String {
    val cleanHost = host.trim()
    val parts = cleanHost.split(".")
    return if (parts.size == 4) {
        "${parts[0]}.${parts[1]}.***.***:$port"
    } else if (cleanHost.contains(":")) {
        val ipv6Parts = cleanHost.split(":")
        if (ipv6Parts.size >= 3) {
            "${ipv6Parts[0]}:${ipv6Parts[1]}:****:$port"
        } else {
            "****:$port"
        }
    } else {
        if (cleanHost.length > 4) "${cleanHost.take(3)}***:$port" else "***:$port"
    }
}

