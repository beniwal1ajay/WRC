package com.example.ward.transport

import com.example.ward.protocol.Header
import com.example.ward.protocol.WardMessage

/**
 * Common interface for all WARD network transports (LAN, Direct P2P, Relay).
 */
interface WardTransport {
    val transportType: TransportType
    val isConnected: Boolean

    fun connect(host: String, port: Int)
    fun sendInput(msg: WardMessage)
    fun sendKeyPress(keycode: Int)
    fun requestKeyframe()
    fun receiveHeader(): Header
    fun receivePayload(length: Int): ByteArray
    fun disconnect()
}
