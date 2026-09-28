package com.example.ward.transport

/**
 * Supported WARD network transports.
 */
enum class TransportType(val displayName: String) {
    LAN("LAN"),
    DIRECT_P2P("Direct P2P"),
    RELAY("Relay")
}
