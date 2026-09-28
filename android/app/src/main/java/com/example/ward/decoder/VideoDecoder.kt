package com.example.ward.decoder

import android.media.MediaCodec
import android.media.MediaCodec.BufferInfo
import android.media.MediaFormat
import android.util.Log
import android.view.Surface
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

private const val TAG = "VideoDecoder"
private const val MIME_TYPE = "video/avc"

class VideoDecoder(
    private val surface: Surface,
    private val onFpsUpdate: (fps: Int) -> Unit = {}
) {
    private var codec: MediaCodec? = null
    private val isRunning = AtomicBoolean(false)
    private var outputThread: Thread? = null
    private val frameCounter = AtomicInteger(0)
    private var lastFpsTime = System.currentTimeMillis()

    @Synchronized
    fun start(width: Int, height: Int) {
        stop()

        try {
            Log.i(TAG, "Initializing MediaCodec H.264 decoder for ${width}x${height}...")
            val format = MediaFormat.createVideoFormat(MIME_TYPE, width, height).apply {
                // Low latency decoding flags
                setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                setInteger(MediaFormat.KEY_PRIORITY, 0) // Real-time priority
            }

            val decoder = MediaCodec.createDecoderByType(MIME_TYPE)
            decoder.configure(format, surface, null, 0)
            decoder.start()
            codec = decoder
            isRunning.set(true)

            // Start dedicated output draining loop
            outputThread = Thread({ drainOutput(decoder) }, "WARD-Decoder-Output").apply {
                priority = Thread.MAX_PRIORITY
                start()
            }

            Log.i(TAG, "MediaCodec decoder started successfully")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start MediaCodec decoder: ${e.message}", e)
        }
    }

    fun feedFrame(data: ByteArray, timestampNs: Long, isKeyframe: Boolean) {
        val decoder = codec ?: return
        if (!isRunning.get()) return

        try {
            val inputIndex = decoder.dequeueInputBuffer(10_000) // 10ms timeout
            if (inputIndex >= 0) {
                val inputBuffer = decoder.getInputBuffer(inputIndex)
                if (inputBuffer != null) {
                    inputBuffer.clear()
                    inputBuffer.put(data)
                    val flags = if (isKeyframe) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
                    decoder.queueInputBuffer(inputIndex, 0, data.size, timestampNs / 1000, flags)
                }
            } else {
                Log.w(TAG, "Dropped frame: input buffer unavailable")
            }
        } catch (e: Exception) {
            Log.w(TAG, "Error feeding frame to decoder: ${e.message}")
        }
    }

    private fun drainOutput(decoder: MediaCodec) {
        val info = BufferInfo()

        while (isRunning.get()) {
            try {
                val outIndex = decoder.dequeueOutputBuffer(info, 10_000)
                when {
                    outIndex >= 0 -> {
                        // Render directly to the surface
                        decoder.releaseOutputBuffer(outIndex, true)

                        val count = frameCounter.incrementAndGet()
                        val now = System.currentTimeMillis()
                        if (now - lastFpsTime >= 1000) {
                            val elapsedSec = (now - lastFpsTime) / 1000.0
                            val fps = (count / elapsedSec).toInt()
                            onFpsUpdate(fps)
                            frameCounter.set(0)
                            lastFpsTime = now
                        }
                    }
                    outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        val newFormat = decoder.outputFormat
                        Log.i(TAG, "Decoder output format changed: $newFormat")
                    }
                    outIndex == MediaCodec.INFO_TRY_AGAIN_LATER -> {
                        // No output available yet
                    }
                }
            } catch (e: Exception) {
                if (isRunning.get()) {
                    Log.w(TAG, "Error draining decoder output: ${e.message}")
                }
                break
            }
        }
    }

    @Synchronized
    fun stop() {
        if (!isRunning.getAndSet(false)) return

        try {
            outputThread?.interrupt()
            outputThread?.join(500)
            outputThread = null

            codec?.stop()
            codec?.release()
            codec = null
            Log.i(TAG, "MediaCodec decoder stopped and released")
        } catch (e: Exception) {
            Log.w(TAG, "Error stopping decoder: ${e.message}")
        }
    }
}
