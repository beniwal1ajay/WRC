package com.example.ward.network

import org.junit.Assert.*
import org.junit.Test

class WardClientReconnectTest {

    @Test
    fun testBackoffProgressionWithoutJitter() {
        val initialDelay = 500L
        val maxDelay = 8000L
        val multiplier = 2.0
        val jitterMs = 0L

        // Attempt 1: 500ms
        assertEquals(500L, computeBackoffDelay(1, initialDelay, maxDelay, multiplier, jitterMs, 0.0))

        // Attempt 2: 500 * 2^1 = 1000ms
        assertEquals(1000L, computeBackoffDelay(2, initialDelay, maxDelay, multiplier, jitterMs, 0.0))

        // Attempt 3: 500 * 2^2 = 2000ms
        assertEquals(2000L, computeBackoffDelay(3, initialDelay, maxDelay, multiplier, jitterMs, 0.0))

        // Attempt 4: 500 * 2^3 = 4000ms
        assertEquals(4000L, computeBackoffDelay(4, initialDelay, maxDelay, multiplier, jitterMs, 0.0))

        // Attempt 5: 500 * 2^4 = 8000ms (capped at maxDelay)
        assertEquals(8000L, computeBackoffDelay(5, initialDelay, maxDelay, multiplier, jitterMs, 0.0))

        // Attempt 6+: still capped at 8000ms
        assertEquals(8000L, computeBackoffDelay(6, initialDelay, maxDelay, multiplier, jitterMs, 0.0))
        assertEquals(8000L, computeBackoffDelay(10, initialDelay, maxDelay, multiplier, jitterMs, 0.0))
    }

    @Test
    fun testBackoffJitterBounds() {
        val initialDelay = 500L
        val maxDelay = 8000L
        val multiplier = 2.0
        val jitterMs = 250L

        for (attempt in 1..10) {
            val minDelay = computeBackoffDelay(attempt, initialDelay, maxDelay, multiplier, jitterMs, 0.0)
            val midDelay = computeBackoffDelay(attempt, initialDelay, maxDelay, multiplier, jitterMs, 0.5)
            val maxJitterDelay = computeBackoffDelay(attempt, initialDelay, maxDelay, multiplier, jitterMs, 1.0)

            assertTrue("minDelay should be <= midDelay", minDelay <= midDelay)
            assertTrue("midDelay should be <= maxJitterDelay", midDelay <= maxJitterDelay)
            assertTrue("jitter range should not exceed jitterMs", maxJitterDelay - minDelay <= jitterMs)
        }
    }

    @Test
    fun testConnectionStateReconnectingDataClass() {
        val state = ConnectionState.Reconnecting(attempt = 3, delayMs = 2150L, reason = "Dead link detected")
        assertEquals(3, state.attempt)
        assertEquals(2150L, state.delayMs)
        assertEquals("Dead link detected", state.reason)
    }
}
