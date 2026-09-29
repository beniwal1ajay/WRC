package com.example.ward.ice

import android.util.Log
import com.example.ward.protocol.IceCandidate
import com.example.ward.stun.StunClient
import com.example.ward.turn.TurnAllocation
import com.example.ward.turn.TurnClient
import com.example.ward.turn.TurnConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.security.SecureRandom

private const val TAG = "IceAgent"

const val CANDIDATE_TYPE_HOST: Byte = 1
const val CANDIDATE_TYPE_SRFLX: Byte = 2
const val CANDIDATE_TYPE_RELAY: Byte = 3

const val PROTOCOL_UDP: Byte = 1
const val PROTOCOL_TCP: Byte = 2

fun calculateCandidatePriority(candidateType: Byte, localPref: Int, componentId: Int): Long {
    val typePref: Long = when (candidateType) {
        CANDIDATE_TYPE_HOST -> 126L
        CANDIDATE_TYPE_SRFLX -> 100L
        CANDIDATE_TYPE_RELAY -> 0L
        else -> 50L
    }
    return (typePref shl 24) or ((localPref.toLong() and 0xFFFFL) shl 8) or (256L - componentId)
}

fun calculatePairPriority(controllingPrio: Long, controlledPrio: Long): Long {
    val minGd = minOf(controllingPrio, controlledPrio)
    val maxGd = maxOf(controllingPrio, controlledPrio)
    val s = if (controllingPrio > controlledPrio) 1L else 0L
    return (minGd shl 32) + (2 * maxGd) + s
}

data class CandidatePair(
    val local: IceCandidate,
    val remote: IceCandidate,
    val priority: Long
)

class IceAgent(
    val socket: DatagramSocket = DatagramSocket(),
    val isControlling: Boolean = false
) {
    val localCandidates = ArrayList<IceCandidate>()
    val remoteCandidates = ArrayList<IceCandidate>()
    var turnAllocation: TurnAllocation? = null
    private val random = SecureRandom()

    val localPort: Int
        get() = socket.localPort

    /**
     * Gather local host, srflx, and relay candidates.
     */
    fun gatherCandidates(
        stunServerHost: String = "stun.l.google.com",
        stunServerPort: Int = 19302,
        turnConfig: TurnConfig? = null
    ): List<IceCandidate> {
        val candidates = ArrayList<IceCandidate>()

        // 1. Host candidates
        try {
            val interfaces = NetworkInterface.getNetworkInterfaces()
            var hostIdx = 0
            while (interfaces != null && interfaces.hasMoreElements()) {
                val iface = interfaces.nextElement()
                if (!iface.isUp) continue

                val addrs = iface.inetAddresses
                while (addrs.hasMoreElements()) {
                    val addr = addrs.nextElement()
                    if (addr is Inet4Address) {
                        val isLoopback = addr.isLoopbackAddress
                        val localPref = if (isLoopback) 1000 else (65535 - hostIdx * 100)
                        hostIdx++
                        val priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, localPref, 1)
                        candidates.add(
                            IceCandidate(
                                candidateType = CANDIDATE_TYPE_HOST,
                                protocol = PROTOCOL_UDP,
                                ip = addr.hostAddress ?: "127.0.0.1",
                                port = localPort,
                                priority = priority
                            )
                        )
                    }
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Error enumerating network interfaces: ${e.message}")
        }

        if (candidates.isEmpty()) {
            candidates.add(
                IceCandidate(
                    candidateType = CANDIDATE_TYPE_HOST,
                    protocol = PROTOCOL_UDP,
                    ip = "127.0.0.1",
                    port = localPort,
                    priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
                )
            )
        }

        // 2. Srflx candidate via STUN
        try {
            val stunAddr = InetAddress.getByName(stunServerHost)
            val tid = ByteArray(12)
            random.nextBytes(tid)
            val req = StunClient.createBindingRequest(tid)

            val packet = DatagramPacket(req, req.size, stunAddr, stunServerPort)
            socket.send(packet)

            val origTimeout = socket.soTimeout
            socket.soTimeout = 2000
            try {
                val recvBuf = ByteArray(1024)
                val recvPacket = DatagramPacket(recvBuf, recvBuf.size)
                socket.receive(recvPacket)
                val mapped = StunClient.parseBindingResponse(
                    recvPacket.data.copyOf(recvPacket.length),
                    tid
                )
                if (mapped != null) {
                    val priority = calculateCandidatePriority(CANDIDATE_TYPE_SRFLX, 65535, 1)
                    val srflxCand = IceCandidate(
                        candidateType = CANDIDATE_TYPE_SRFLX,
                        protocol = PROTOCOL_UDP,
                        ip = mapped.first,
                        port = mapped.second,
                        priority = priority
                    )
                    candidates.add(srflxCand)
                    Log.i(TAG, "Gathered srflx candidate: ${mapped.first}:${mapped.second}")
                }
            } finally {
                socket.soTimeout = origTimeout
            }
        } catch (e: Exception) {
            Log.d(TAG, "Srflx gathering failed: ${e.message}")
        }

        // 3. Relay candidate via TURN
        if (turnConfig != null) {
            try {
                val client = TurnClient(socket)
                val alloc = client.allocate(turnConfig)
                if (alloc != null) {
                    turnAllocation = alloc
                    val priority = calculateCandidatePriority(CANDIDATE_TYPE_RELAY, 65535, 1)
                    val relayCand = IceCandidate(
                        candidateType = CANDIDATE_TYPE_RELAY,
                        protocol = PROTOCOL_UDP,
                        ip = alloc.relayedIp,
                        port = alloc.relayedPort,
                        priority = priority
                    )
                    candidates.add(relayCand)
                    Log.i(TAG, "Gathered Relay candidate: ${alloc.relayedIp}:${alloc.relayedPort}")
                }
            } catch (e: Exception) {
                Log.w(TAG, "Relay candidate gathering failed: ${e.message}")
            }
        }

        localCandidates.clear()
        localCandidates.addAll(candidates)
        return candidates
    }

    suspend fun gatherCandidatesAsync(
        stunServerHost: String = "stun.l.google.com",
        stunServerPort: Int = 19302,
        turnConfig: TurnConfig? = null
    ): List<IceCandidate> = withContext(Dispatchers.IO) {
        gatherCandidates(stunServerHost, stunServerPort, turnConfig)
    }

    fun setRemoteCandidates(candidates: List<IceCandidate>) {
        remoteCandidates.clear()
        remoteCandidates.addAll(candidates)
    }

    fun formCandidatePairs(): List<CandidatePair> {
        val pairs = ArrayList<CandidatePair>()
        for (local in localCandidates) {
            for (remote in remoteCandidates) {
                if (local.protocol != remote.protocol) continue

                val priority = if (isControlling) {
                    calculatePairPriority(local.priority, remote.priority)
                } else {
                    calculatePairPriority(remote.priority, local.priority)
                }
                pairs.add(CandidatePair(local, remote, priority))
            }
        }
        pairs.sortByDescending { it.priority }
        return pairs
    }

    fun checkPairConnectivity(pair: CandidatePair, timeoutMs: Int = 2000): Boolean {
        try {
            val destAddr = InetAddress.getByName(pair.remote.ip)
            val tid = ByteArray(12)
            random.nextBytes(tid)
            val req = StunClient.createBindingRequest(tid)

            val packet = DatagramPacket(req, req.size, destAddr, pair.remote.port)
            socket.send(packet)

            val origTimeout = socket.soTimeout
            socket.soTimeout = timeoutMs
            val startTime = System.currentTimeMillis()

            try {
                val buf = ByteArray(1024)
                while (System.currentTimeMillis() - startTime < timeoutMs) {
                    val recvPacket = DatagramPacket(buf, buf.size)
                    socket.receive(recvPacket)

                    val resp = StunClient.parseBindingResponse(
                        recvPacket.data.copyOf(recvPacket.length),
                        tid
                    )
                    if (resp != null) {
                        Log.i(TAG, "ICE check succeeded for pair: ${pair.remote.ip}:${pair.remote.port}")
                        return true
                    }
                }
            } finally {
                socket.soTimeout = origTimeout
            }
        } catch (e: Exception) {
            Log.d(TAG, "ICE check failed for ${pair.remote.ip}:${pair.remote.port}: ${e.message}")
        }
        return false
    }

    suspend fun performIceChecksAsync(timeoutPerPairMs: Int = 2000): CandidatePair? = withContext(Dispatchers.IO) {
        val pairs = formCandidatePairs()
        Log.i(TAG, "Starting ICE checks across ${pairs.size} pairs...")
        for (pair in pairs) {
            if (checkPairConnectivity(pair, timeoutPerPairMs)) {
                Log.i(TAG, "Nominated pair: local=${pair.local.ip}:${pair.local.port}, remote=${pair.remote.ip}:${pair.remote.port}")
                return@withContext pair
            }
        }
        null
    }

    fun close() {
        if (!socket.isClosed) {
            socket.close()
        }
    }
}
