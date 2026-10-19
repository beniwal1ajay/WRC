package com.example.ward.service

import android.content.Context
import android.util.Log
import android.view.Surface
import com.example.ward.data.PairedDevice
import com.example.ward.data.PairedDeviceStore
import com.example.ward.decoder.VideoDecoder
import com.example.ward.network.ConnectionState
import com.example.ward.network.SessionStats
import com.example.ward.network.WardClient
import com.example.ward.protocol.WardMessage
import com.example.ward.network.CandidateEndpoint
import com.example.ward.network.ConnectionTier
import com.example.ward.transport.LanTlsTransport
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

private const val TAG = "WardSessionManager"

/**
 * Singleton manager retaining the lifecycle of the active remote desktop session.
 * Survives Activity destruction, orientation rotation, and app backgrounding.
 */
object WardSessionManager {
    private val _connectionState = MutableStateFlow<ConnectionState>(ConnectionState.Disconnected)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private val _fps = MutableStateFlow(0)
    val fps: StateFlow<Int> = _fps.asStateFlow()

    private val _streamWidth = MutableStateFlow(1920)
    val streamWidth: StateFlow<Int> = _streamWidth.asStateFlow()

    private val _streamHeight = MutableStateFlow(1080)
    val streamHeight: StateFlow<Int> = _streamHeight.asStateFlow()

    private val _targetFps = MutableStateFlow(45)
    val targetFps: StateFlow<Int> = _targetFps.asStateFlow()

    private val _sessionStats = MutableStateFlow<SessionStats?>(null)
    val sessionStats: StateFlow<SessionStats?> = _sessionStats.asStateFlow()

    private val _activeHost = MutableStateFlow("")
    val activeHost: StateFlow<String> = _activeHost.asStateFlow()

    private val _displays = MutableStateFlow<List<WardMessage.DisplayInfo>>(emptyList())
    val displays: StateFlow<List<WardMessage.DisplayInfo>> = _displays.asStateFlow()

    private val _activeDisplayId = MutableStateFlow(0)
    val activeDisplayId: StateFlow<Int> = _activeDisplayId.asStateFlow()

    private var wardClient: WardClient? = null
    private var videoDecoder: VideoDecoder? = null

    @Synchronized
    fun connect(
        context: Context,
        host: String,
        port: Int,
        pairedDevice: PairedDevice,
        deviceStore: PairedDeviceStore,
        onPinRequired: ((String) -> Unit) -> Unit,
        onDeviceUpdated: ((PairedDevice) -> Unit)? = null
    ) {
        val currentState = _connectionState.value
        if (currentState is ConnectionState.Connected || currentState is ConnectionState.Connecting) {
            Log.i(TAG, "Connect called while already connecting/connected; ignoring redundant call")
            return
        }

        _activeHost.value = host
        _connectionState.value = ConnectionState.Connecting

        // Pre-create decoder
        videoDecoder = VideoDecoder { newFps ->
            _fps.value = newFps
        }

        val candidatesProvider = {
            val list = mutableListOf<CandidateEndpoint>()
            list.add(CandidateEndpoint(ConnectionTier.LAN_DIRECT, host, port, "Active Target"))
            list
        }

        val transport = LanTlsTransport(
            pinnedCertProvider = { deviceStore.getPinnedCertSha256() },
            onCertDiscovered = { fp -> deviceStore.setPinnedCertSha256(fp) }
        )

        val client = WardClient(
            transport = transport,
            onStateChanged = { newState ->
                _connectionState.value = newState
                try {
                    if (newState is ConnectionState.Connected) {
                        WardRemoteSessionService.start(context.applicationContext, host)
                    } else if (newState is ConnectionState.Disconnected) {
                        WardRemoteSessionService.stop(context.applicationContext)
                    }
                } catch (e: Throwable) {
                    Log.w(TAG, "WardRemoteSessionService toggle error: ${e.message}")
                }
            },
            onVideoConfig = { width, height, fps ->
                _streamWidth.value = width
                _streamHeight.value = height
                if (fps > 0) _targetFps.value = fps
                videoDecoder?.start(width, height)
            },
            onVideoFrame = { data, timestampNs, isKeyframe ->
                videoDecoder?.feedFrame(data, timestampNs, isKeyframe)
            },
            onDisplayList = { displayList ->
                _displays.value = displayList.displays
                _activeDisplayId.value = displayList.activeId
                Log.i(TAG, "Updated available displays: ${displayList.displays.map { "${it.id}:${it.name}" }}, active=${displayList.activeId}")
            },
            onSessionAccepted = { accepted ->
                val updated = deviceStore.updateFromSession(accepted, host, port)
                onDeviceUpdated?.invoke(updated)
            },
            onStatsUpdated = { stats ->
                _sessionStats.value = stats
            },
            clientDeviceId = deviceStore.getOrCreateClientDeviceId(),
            authToken = deviceStore.getAuthToken(),
            onPinRequired = onPinRequired,
            onTokenInvalidated = {
                deviceStore.saveAuthToken("")
            },
            candidateEndpointsProvider = candidatesProvider,
            autoReconnect = true
        )

        wardClient = client
        client.connect(host, port)
    }

    @Synchronized
    fun disconnect(context: Context) {
        wardClient?.disconnect()
        wardClient = null

        videoDecoder?.stop()
        videoDecoder = null

        _connectionState.value = ConnectionState.Disconnected
        _fps.value = 0
        _displays.value = emptyList()
        _activeDisplayId.value = 0
        WardRemoteSessionService.stop(context.applicationContext)
    }

    @Synchronized
    fun attachSurface(surface: Surface) {
        videoDecoder?.attachSurface(surface)
        wardClient?.requestKeyframe()
    }

    @Synchronized
    fun detachSurface() {
        videoDecoder?.detachSurface()
    }

    fun sendInput(msg: WardMessage) {
        wardClient?.sendInput(msg)
    }

    fun sendKeyPress(keysym: Int) {
        wardClient?.sendKeyPress(keysym)
    }

    fun requestKeyframe() {
        wardClient?.requestKeyframe()
    }

    fun requestStreamConfig(width: Int, height: Int, fps: Int) {
        _streamWidth.value = width
        _streamHeight.value = height
        _targetFps.value = fps
        wardClient?.sendInput(WardMessage.VideoConfig(width, height, 1, fps.toShort()))
        Log.i(TAG, "Requested stream configuration: ${width}x${height} @ ${fps} FPS")
    }

    fun selectDisplay(displayId: Int) {
        _activeDisplayId.value = displayId
        wardClient?.selectDisplay(displayId)
        Log.i(TAG, "Requested display switch to ID $displayId")
    }
}
