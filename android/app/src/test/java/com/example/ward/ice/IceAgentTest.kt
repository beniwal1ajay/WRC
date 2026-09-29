package com.example.ward.ice

import com.example.ward.protocol.IceCandidate
import com.example.ward.stun.StunClient
import org.junit.Assert.*
import org.junit.Test
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import kotlin.concurrent.thread

class IceAgentTest {

    @Test
    fun testCandidatePriorityOrdering() {
        val hostPrio = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
        val srflxPrio = calculateCandidatePriority(CANDIDATE_TYPE_SRFLX, 65535, 1)
        val relayPrio = calculateCandidatePriority(CANDIDATE_TYPE_RELAY, 65535, 1)

        assertTrue(hostPrio > srflxPrio)
        assertTrue(srflxPrio > relayPrio)
    }

    @Test
    fun testCandidatePairFormationAndSorting() {
        val socket = DatagramSocket()
        val agent = IceAgent(socket, isControlling = true)

        try {
            val localHost = IceCandidate(
                candidateType = CANDIDATE_TYPE_HOST,
                protocol = PROTOCOL_UDP,
                ip = "192.168.1.10",
                port = 5000,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
            )
            val localSrflx = IceCandidate(
                candidateType = CANDIDATE_TYPE_SRFLX,
                protocol = PROTOCOL_UDP,
                ip = "203.0.113.1",
                port = 5000,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_SRFLX, 65535, 1)
            )

            val remoteHost = IceCandidate(
                candidateType = CANDIDATE_TYPE_HOST,
                protocol = PROTOCOL_UDP,
                ip = "192.168.1.20",
                port = 6000,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
            )
            val remoteSrflx = IceCandidate(
                candidateType = CANDIDATE_TYPE_SRFLX,
                protocol = PROTOCOL_UDP,
                ip = "203.0.113.2",
                port = 6000,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_SRFLX, 65535, 1)
            )

            agent.localCandidates.addAll(listOf(localHost, localSrflx))
            agent.setRemoteCandidates(listOf(remoteHost, remoteSrflx))

            val pairs = agent.formCandidatePairs()
            assertEquals(4, pairs.size)

            // Top pair must be Host <-> Host
            assertEquals(CANDIDATE_TYPE_HOST, pairs[0].local.candidateType)
            assertEquals(CANDIDATE_TYPE_HOST, pairs[0].remote.candidateType)
            assertTrue(pairs[0].priority >= pairs[1].priority)
        } finally {
            agent.close()
        }
    }

    @Test
    fun testConnectivityCheckOverLoopback() {
        val socket1 = DatagramSocket(0, InetAddress.getByName("127.0.0.1"))
        val socket2 = DatagramSocket(0, InetAddress.getByName("127.0.0.1"))

        val agent1 = IceAgent(socket1, isControlling = true)

        // Responder thread on socket2
        val responder = thread {
            try {
                val buf = ByteArray(1024)
                val packet = DatagramPacket(buf, buf.size)
                socket2.receive(packet)

                val tid = packet.data.copyOfRange(8, 20)
                val respBytes = StunClient.createBindingResponse(tid, "127.0.0.1", packet.port)
                val respPacket = DatagramPacket(respBytes, respBytes.size, packet.address, packet.port)
                socket2.send(respPacket)
            } catch (_: Exception) {}
        }

        try {
            val localCand = IceCandidate(
                candidateType = CANDIDATE_TYPE_HOST,
                protocol = PROTOCOL_UDP,
                ip = "127.0.0.1",
                port = socket1.localPort,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
            )
            val remoteCand = IceCandidate(
                candidateType = CANDIDATE_TYPE_HOST,
                protocol = PROTOCOL_UDP,
                ip = "127.0.0.1",
                port = socket2.localPort,
                priority = calculateCandidatePriority(CANDIDATE_TYPE_HOST, 65535, 1)
            )

            val pair = CandidatePair(localCand, remoteCand, calculatePairPriority(localCand.priority, remoteCand.priority))
            val success = agent1.checkPairConnectivity(pair, 2000)
            assertTrue("Loopback ICE connectivity check must succeed", success)
        } finally {
            agent1.close()
            socket2.close()
            responder.join(1000)
        }
    }
}
