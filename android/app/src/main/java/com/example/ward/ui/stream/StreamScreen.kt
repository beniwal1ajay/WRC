package com.example.ward.ui.stream

import android.content.Context
import android.text.InputType
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import android.view.InputDevice
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import com.example.ward.MainActivity
import com.example.ward.data.PairedDevice
import com.example.ward.data.PairedDeviceStore
import com.example.ward.decoder.VideoDecoder
import com.example.ward.network.ConnectionState
import com.example.ward.network.SessionStats
import com.example.ward.network.WardClient
import com.example.ward.protocol.WardMessage
import com.example.ward.service.WardSessionManager
import com.example.ward.transport.TransportType
import com.example.ward.transport.maskIpAddress
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress
import java.net.Socket

// Linux X11/XKB keysyms
private const val KEYSYM_FLAG = 0x80000000.toInt()
private const val KEYSYM_BACKSPACE = 0xFF08
private const val KEYSYM_TAB = 0xFF09
private const val KEYSYM_RETURN = 0xFF0D
private const val KEYSYM_ESCAPE = 0xFF1B
private const val KEYSYM_SPACE = 0x0020
private const val KEYSYM_PAGE_UP = 0xFF55
private const val KEYSYM_PAGE_DOWN = 0xFF56
private const val KEYSYM_LEFT = 0xFF51
private const val KEYSYM_UP = 0xFF52
private const val KEYSYM_RIGHT = 0xFF53
private const val KEYSYM_DOWN = 0xFF54
private const val KEYSYM_SUPER = 0xFFEB

// Linux evdev keycodes
private const val EVDEV_KEY_LEFTCTRL = 29
private const val EVDEV_KEY_A = 30
private const val EVDEV_KEY_C = 46
private const val EVDEV_KEY_V = 47
private const val EVDEV_KEY_Z = 44

/**
 * Invisible native Android View providing direct IME InputConnection without
 * accumulating or desynchronizing Compose string buffers. Prevents sticky repeat keys.
 */
class RemoteInputView(
    context: Context,
    private val onChar: (Char) -> Unit,
    private val onBackspace: () -> Unit
) : View(context) {

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
                InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        outAttrs.imeOptions = EditorInfo.IME_ACTION_NONE or EditorInfo.IME_FLAG_NO_FULLSCREEN

        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                if (text != null) {
                    for (ch in text) {
                        onChar(ch)
                    }
                }
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                repeat(beforeLength) {
                    onBackspace()
                }
                return true
            }

            override fun sendKeyEvent(event: KeyEvent?): Boolean {
                if (event == null) return false
                if (event.action == KeyEvent.ACTION_DOWN) {
                    when (event.keyCode) {
                        KeyEvent.KEYCODE_DEL -> {
                            onBackspace()
                            return true
                        }
                        KeyEvent.KEYCODE_ENTER -> {
                            onChar('\n')
                            return true
                        }
                        else -> {
                            val unicode = event.unicodeChar
                            if (unicode != 0) {
                                onChar(unicode.toChar())
                                return true
                            }
                        }
                    }
                } else if (event.action == KeyEvent.ACTION_UP) {
                    when (event.keyCode) {
                        KeyEvent.KEYCODE_DEL, KeyEvent.KEYCODE_ENTER -> return true
                        else -> {
                            if (event.unicodeChar != 0) return true
                        }
                    }
                }
                return super.sendKeyEvent(event)
            }
        }
    }
}

private tailrec fun Context.findMainActivity(): MainActivity? = when (this) {
    is MainActivity -> this
    is android.content.ContextWrapper -> baseContext?.findMainActivity() ?: MainActivity.currentInstance
    else -> MainActivity.currentInstance
}

@Composable
fun WardRemoteApp() {
    val context = LocalContext.current
    val deviceStore = remember { PairedDeviceStore(context) }
    var pairedDevice by remember { mutableStateOf(deviceStore.getPairedDevice()) }

    var host by remember { mutableStateOf(pairedDevice.lastKnownHost) }
    var portText by remember { mutableStateOf(pairedDevice.lastKnownPort.toString()) }
    var showManualSettings by remember { mutableStateOf(false) }
    var isOnline by remember { mutableStateOf<Boolean?>(null) }
    var pendingPinCallback by remember { mutableStateOf<((String) -> Unit)?>(null) }
    var enteredPinText by remember { mutableStateOf("") }

    val connectionState by WardSessionManager.connectionState.collectAsState()
    val fps by WardSessionManager.fps.collectAsState()
    val targetFps by WardSessionManager.targetFps.collectAsState()
    val streamWidth by WardSessionManager.streamWidth.collectAsState()
    val streamHeight by WardSessionManager.streamHeight.collectAsState()
    val sessionStats by WardSessionManager.sessionStats.collectAsState()
    var showShortcuts by remember { mutableStateOf(false) }
    var showDiagnostics by remember { mutableStateOf(false) }
    var showQualityDialog by remember { mutableStateOf(false) }

    val prefRes = remember { deviceStore.getPreferredResolution() }
    val prefFpsVal = remember { deviceStore.getPreferredFps() }
    var selectedResWidth by remember { mutableIntStateOf(prefRes.first) }
    var selectedResHeight by remember { mutableIntStateOf(prefRes.second) }
    var selectedFps by remember { mutableIntStateOf(prefFpsVal) }

    // Auto-contracting bar state & interaction timer
    var isBarCollapsed by remember { mutableStateOf(false) }
    var lastBarInteraction by remember { mutableLongStateOf(System.currentTimeMillis()) }

    var remoteInputView: RemoteInputView? by remember { mutableStateOf(null) }

    // Manage screen keep-awake and fullscreen immersive mode based on active connection
    val activity = remember(context) { context.findMainActivity() }
    DisposableEffect(connectionState) {
        if (connectionState is ConnectionState.Connected) {
            activity?.setKeepScreenOn(true)
            activity?.setImmersiveMode(true)
        } else {
            activity?.setKeepScreenOn(false)
            activity?.setImmersiveMode(false)
        }
        onDispose {
            activity?.setKeepScreenOn(false)
            activity?.setImmersiveMode(false)
        }
    }

    // Re-assert immersive mode on orientation flip and continuously while connected
    val screenWidth = LocalConfiguration.current.screenWidthDp
    val screenHeight = LocalConfiguration.current.screenHeightDp
    val isLandscapeOrientation = screenWidth > screenHeight
    LaunchedEffect(isLandscapeOrientation, connectionState) {
        if (connectionState is ConnectionState.Connected) {
            activity?.setImmersiveMode(true)
        }
    }

    // Intercept back gesture/button during active session: dismiss overlays first, or disconnect cleanly
    if (connectionState is ConnectionState.Connected) {
        BackHandler {
            if (showDiagnostics || showQualityDialog || showShortcuts) {
                showDiagnostics = false
                showQualityDialog = false
                showShortcuts = false
            } else {
                WardSessionManager.disconnect(context)
            }
        }
    }

    // Apply preferred stream resolution and framerate immediately when connection is established
    LaunchedEffect(connectionState) {
        if (connectionState is ConnectionState.Connected) {
            val (w, h) = deviceStore.getPreferredResolution()
            val f = deviceStore.getPreferredFps()
            WardSessionManager.requestStreamConfig(w, h, f)
        }
    }

    // Probe host reachability periodically ONLY when idle in Disconnected state
    LaunchedEffect(host, portText, connectionState) {
        if (connectionState is ConnectionState.Disconnected) {
            val targetHost = host.trim()
            val targetPort = portText.toIntOrNull() ?: 9740
            if (targetHost.isNotEmpty()) {
                while (connectionState is ConnectionState.Disconnected) {
                    val reachable = withContext(Dispatchers.IO) {
                        try {
                            Socket().use { s ->
                                s.soTimeout = 1000
                                s.connect(InetSocketAddress(targetHost, targetPort), 1000)
                                true
                            }
                        } catch (e: Exception) {
                            false
                        }
                    }
                    isOnline = reachable
                    delay(10000)
                }
            }
        }
    }

    fun sendKeysym(keysym: Int) {
        val code = keysym or KEYSYM_FLAG
        WardSessionManager.sendKeyPress(code)
    }

    fun sendCtrlKey(evdevKey: Int) {
        WardSessionManager.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
        WardSessionManager.sendInput(WardMessage.KeyDown(evdevKey))
        WardSessionManager.sendInput(WardMessage.KeyUp(evdevKey))
        WardSessionManager.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
    }

    fun showKeyboard() {
        remoteInputView?.let { view ->
            view.requestFocus()
            val imm = view.context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            imm?.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
        }
    }

    // Auto-contract bar after 8 seconds of inactivity
    LaunchedEffect(isBarCollapsed, lastBarInteraction) {
        if (!isBarCollapsed) {
            delay(8000)
            isBarCollapsed = true
        }
    }

    BoxWithConstraints(modifier = Modifier.fillMaxSize().background(Color.Black)) {
        val screenWidth = maxWidth
        val screenHeight = maxHeight
        val isLandscape = screenWidth > screenHeight

        when (val state = connectionState) {
            is ConnectionState.Connected -> {
                // 1. Aspect-ratio preserved video surface
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    val containerAspect = screenWidth / screenHeight
                    val videoAspect = if (streamWidth > 0 && streamHeight > 0) {
                        streamWidth.toFloat() / streamHeight.toFloat()
                    } else {
                        16f / 9f
                    }

                    val videoModifier = if (containerAspect > videoAspect) {
                        Modifier.fillMaxHeight().aspectRatio(videoAspect)
                    } else {
                        Modifier.fillMaxWidth().aspectRatio(videoAspect)
                    }

                    AndroidView(
                        modifier = videoModifier,
                        factory = { ctx ->
                            SurfaceView(ctx).apply {
                                holder.addCallback(object : SurfaceHolder.Callback {
                                    override fun surfaceCreated(holder: SurfaceHolder) {
                                        WardSessionManager.attachSurface(holder.surface)
                                    }

                                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

                                    override fun surfaceDestroyed(holder: SurfaceHolder) {
                                        WardSessionManager.detachSurface()
                                    }
                                })
                            }
                        }
                    )
                }

                // 2. Full-screen Multi-Gesture Touchpad Overlay with System UI boundaries & mouse support
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
                    factory = { ctx ->
                        android.view.View(ctx).apply {
                            // Touch tracking variables
                            var lastX1 = 0f
                            var lastY1 = 0f
                            var lastX2 = 0f
                            var lastY2 = 0f
                            var downX = 0f
                            var downY = 0f
                            var downTime = 0L
                            var pointerCount = 0
                            var isDragging = false
                            var threeFingerSwipeStartY = 0f
                            var accumulatedDx = 0f
                            var accumulatedDy = 0f
                            var lastMoveDispatchTime = 0L

                            // Generic motion listener for physical mouse scroll wheels & hover motion
                            setOnGenericMotionListener { _, event ->
                                if (event.isFromSource(InputDevice.SOURCE_MOUSE) || event.isFromSource(InputDevice.SOURCE_MOUSE_RELATIVE)) {
                                    when (event.action) {
                                        MotionEvent.ACTION_SCROLL -> {
                                            val vScroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL)
                                            val hScroll = event.getAxisValue(MotionEvent.AXIS_HSCROLL)
                                            if (Math.abs(vScroll) > 0.01f) {
                                                val stepY = if (vScroll > 0) -1.0f else 1.0f
                                                WardSessionManager.sendInput(WardMessage.MouseScroll(0f, stepY))
                                            }
                                            if (Math.abs(hScroll) > 0.01f) {
                                                val stepX = if (hScroll > 0) 1.0f else -1.0f
                                                WardSessionManager.sendInput(WardMessage.MouseScroll(stepX, 0f))
                                            }
                                            true
                                        }
                                        MotionEvent.ACTION_HOVER_MOVE -> {
                                            val rx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X)
                                            val ry = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y)
                                            if (rx != 0f || ry != 0f) {
                                                WardSessionManager.sendInput(WardMessage.MouseMove(rx, ry))
                                            }
                                            true
                                        }
                                        else -> false
                                    }
                                } else false
                            }

                            setOnTouchListener { v, event ->
                                pointerCount = event.pointerCount

                                // Physical mouse button events
                                if (event.isFromSource(InputDevice.SOURCE_MOUSE)) {
                                    when (event.actionMasked) {
                                        MotionEvent.ACTION_DOWN -> {
                                            val buttonState = event.buttonState
                                            if ((buttonState and MotionEvent.BUTTON_PRIMARY) != 0) {
                                                WardSessionManager.sendInput(WardMessage.MouseButton(1, 1))
                                            }
                                            if ((buttonState and MotionEvent.BUTTON_SECONDARY) != 0) {
                                                WardSessionManager.sendInput(WardMessage.MouseButton(2, 1))
                                            }
                                            if ((buttonState and MotionEvent.BUTTON_TERTIARY) != 0) {
                                                WardSessionManager.sendInput(WardMessage.MouseButton(3, 1))
                                            }
                                            return@setOnTouchListener true
                                        }
                                        MotionEvent.ACTION_UP -> {
                                            WardSessionManager.sendInput(WardMessage.MouseButton(1, 0))
                                            WardSessionManager.sendInput(WardMessage.MouseButton(2, 0))
                                            WardSessionManager.sendInput(WardMessage.MouseButton(3, 0))
                                            return@setOnTouchListener true
                                        }
                                        MotionEvent.ACTION_MOVE -> {
                                            val rx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X)
                                            val ry = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y)
                                            if (rx != 0f || ry != 0f) {
                                                WardSessionManager.sendInput(WardMessage.MouseMove(rx, ry))
                                            }
                                            return@setOnTouchListener true
                                        }
                                    }
                                }

                                // Virtual touchpad gesture handling
                                when (event.actionMasked) {
                                    MotionEvent.ACTION_DOWN -> {
                                        downX = event.x
                                        downY = event.y
                                        lastX1 = event.x
                                        lastY1 = event.y
                                        accumulatedDx = 0f
                                        accumulatedDy = 0f
                                        lastMoveDispatchTime = System.currentTimeMillis()
                                        downTime = System.currentTimeMillis()
                                        isDragging = false
                                        v.performClick()
                                        true
                                    }

                                    MotionEvent.ACTION_POINTER_DOWN -> {
                                        if (pointerCount == 2) {
                                            lastX1 = event.getX(0)
                                            lastY1 = event.getY(0)
                                            lastX2 = event.getX(1)
                                            lastY2 = event.getY(1)
                                        } else if (pointerCount == 3) {
                                            threeFingerSwipeStartY = event.getY(0)
                                        }
                                        true
                                    }

                                    MotionEvent.ACTION_MOVE -> {
                                        val now = System.currentTimeMillis()

                                        if (pointerCount == 1) {
                                            val dx = event.x - lastX1
                                            val dy = event.y - lastY1
                                            lastX1 = event.x
                                            lastY1 = event.y

                                            accumulatedDx += dx
                                            accumulatedDy += dy

                                            val totalDist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                                            // Enter drag mode if held down for > 350ms
                                            if (!isDragging && now - downTime > 350 && totalDist < 25) {
                                                isDragging = true
                                                WardSessionManager.sendInput(WardMessage.MouseButton(1, 1))
                                            }

                                            // Pace pointer updates to ~100Hz max to avoid D-Bus saturation
                                            if (now - lastMoveDispatchTime >= 10L) {
                                                if (accumulatedDx != 0f || accumulatedDy != 0f) {
                                                    WardSessionManager.sendInput(WardMessage.MouseMove(accumulatedDx, accumulatedDy))
                                                    accumulatedDx = 0f
                                                    accumulatedDy = 0f
                                                }
                                                lastMoveDispatchTime = now
                                            }
                                        } else if (pointerCount == 2) {
                                            // 2-finger scroll
                                            val currentY1 = event.getY(0)
                                            val currentY2 = event.getY(1)
                                            val currentX1 = event.getX(0)
                                            val currentX2 = event.getX(1)

                                            val dy = ((currentY1 - lastY1) + (currentY2 - lastY2)) / 2f
                                            val dx = ((currentX1 - lastX1) + (currentX2 - lastX2)) / 2f

                                            if (Math.abs(dy) > 2f) {
                                                val stepY = if (dy > 0) 1.0f else -1.0f
                                                WardSessionManager.sendInput(WardMessage.MouseScroll(0f, stepY))
                                            }

                                            if (Math.abs(dx) > 4f) {
                                                val stepX = if (dx > 0) 1.0f else -1.0f
                                                WardSessionManager.sendInput(WardMessage.MouseScroll(stepX, 0f))
                                            }

                                            lastX1 = currentX1
                                            lastY1 = currentY1
                                            lastX2 = currentX2
                                            lastY2 = currentY2
                                        } else if (pointerCount == 3) {
                                            // 3-finger swipe up for Super / Windows key
                                            val currentY = event.getY(0)
                                            if (threeFingerSwipeStartY - currentY > 120f) {
                                                sendKeysym(KEYSYM_SUPER)
                                                threeFingerSwipeStartY = currentY
                                            }
                                        }
                                        true
                                    }

                                    MotionEvent.ACTION_POINTER_UP -> {
                                        val duration = System.currentTimeMillis() - downTime
                                        if (pointerCount == 2 && duration < 250) {
                                            WardSessionManager.sendInput(WardMessage.MouseButton(2, 1))
                                            WardSessionManager.sendInput(WardMessage.MouseButton(2, 0))
                                        } else if (pointerCount == 3 && duration < 250) {
                                            WardSessionManager.sendInput(WardMessage.MouseButton(3, 1))
                                            WardSessionManager.sendInput(WardMessage.MouseButton(3, 0))
                                        }
                                        true
                                    }

                                    MotionEvent.ACTION_UP -> {
                                        if (accumulatedDx != 0f || accumulatedDy != 0f) {
                                            WardSessionManager.sendInput(WardMessage.MouseMove(accumulatedDx, accumulatedDy))
                                            accumulatedDx = 0f
                                            accumulatedDy = 0f
                                        }

                                        val duration = System.currentTimeMillis() - downTime
                                        val dist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                                        if (isDragging) {
                                            WardSessionManager.sendInput(WardMessage.MouseButton(1, 0))
                                            isDragging = false
                                        } else if (duration < 220 && dist < 20) {
                                            WardSessionManager.sendInput(WardMessage.MouseButton(1, 1))
                                            WardSessionManager.sendInput(WardMessage.MouseButton(1, 0))
                                        }
                                        true
                                    }

                                    else -> false
                                }
                            }
                        }
                    }
                )

                // 3. Invisible Native Android View: Types directly to PC without buffer desync
                AndroidView(
                    modifier = Modifier
                        .size(1.dp)
                        .alpha(0.001f),
                    factory = { ctx ->
                        RemoteInputView(
                            context = ctx,
                            onChar = { ch ->
                                if (ch == '\n') {
                                    sendKeysym(KEYSYM_RETURN)
                                } else {
                                    sendKeysym(ch.code)
                                }
                            },
                            onBackspace = {
                                sendKeysym(KEYSYM_BACKSPACE)
                            }
                        ).also { remoteInputView = it }
                    }
                )

                // 4. Controls: Shifted to Left in Landscape, Centered at Top in Portrait
                if (isLandscape) {
                    // -------------------------------------------------------------
                    // LANDSCAPE MODE: Shift control bar to LEFT, Scroll buttons on RIGHT
                    // -------------------------------------------------------------

                    // Left Vertical Bar (Expanded)
                    AnimatedVisibility(
                        visible = !isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier.align(Alignment.CenterStart).padding(start = 8.dp)
                    ) {
                        Card(
                            colors = CardDefaults.cardColors(containerColor = Color(0xF0141824)),
                            shape = RoundedCornerShape(16.dp),
                            border = BorderStroke(1.dp, Color(0x33FFFFFF)),
                            elevation = CardDefaults.cardElevation(8.dp)
                        ) {
                            Column(
                                modifier = Modifier.padding(vertical = 10.dp, horizontal = 6.dp),
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                // Collapse toggle button
                                Button(
                                    onClick = { isBarCollapsed = true },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF1E293B), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 6.dp, vertical = 2.dp),
                                    modifier = Modifier.height(26.dp),
                                    shape = RoundedCornerShape(6.dp)
                                ) {
                                    Text("◀", fontSize = 11.sp, color = Color.White)
                                }

                                // Stats: FPS and Resolution & Transport Badge (Click to open Quality Settings)
                                Column(
                                    horizontalAlignment = Alignment.CenterHorizontally,
                                    modifier = Modifier.clickable {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showQualityDialog = !showQualityDialog
                                        if (showQualityDialog) {
                                            showDiagnostics = false
                                            showShortcuts = false
                                        }
                                    }
                                ) {
                                    Surface(
                                        color = if (sessionStats?.connectionType == TransportType.VPN) Color(0xFF0284C7) else Color(0xFF059669),
                                        shape = RoundedCornerShape(4.dp)
                                    ) {
                                        Text(
                                            text = sessionStats?.connectionType?.displayName ?: "LAN",
                                            color = Color.White,
                                            fontSize = 8.sp,
                                            fontWeight = FontWeight.Bold,
                                            modifier = Modifier.padding(horizontal = 4.dp, vertical = 1.dp)
                                        )
                                    }
                                    Spacer(modifier = Modifier.height(2.dp))
                                    Text(
                                        text = "$fps FPS",
                                        color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.Bold
                                    )
                                    Text(
                                        text = "${streamWidth}x${streamHeight}",
                                        color = Color(0xFF94A3B8),
                                        fontSize = 9.sp
                                    )
                                }

                                // Diagnostics Toggle (Closes Keys & Quality)
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showDiagnostics = !showDiagnostics
                                        if (showDiagnostics) {
                                            showShortcuts = false
                                            showQualityDialog = false
                                        }
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (showDiagnostics) Color(0xFF0284C7) else Color(0xFF334155),
                                        contentColor = Color.White
                                    ),
                                    contentPadding = PaddingValues(horizontal = 6.dp, vertical = 2.dp),
                                    modifier = Modifier.height(30.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("📊", fontSize = 13.sp, color = Color.White)
                                }

                                // Quality Settings Toggle (1080p/720p & 60/30 FPS)
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showQualityDialog = !showQualityDialog
                                        if (showQualityDialog) {
                                            showDiagnostics = false
                                            showShortcuts = false
                                        }
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (showQualityDialog) Color(0xFF6366F1) else Color(0xFF334155),
                                        contentColor = Color.White
                                    ),
                                    contentPadding = PaddingValues(horizontal = 6.dp, vertical = 2.dp),
                                    modifier = Modifier.height(30.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("⚙️", fontSize = 13.sp, color = Color.White)
                                }

                                HorizontalDivider(color = Color(0x33FFFFFF), thickness = 1.dp, modifier = Modifier.width(36.dp))

                                // Direct Keyboard Trigger
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showKeyboard()
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                    modifier = Modifier.height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("⌨", fontSize = 15.sp, color = Color.White)
                                }

                                // Shortcuts Toggle (Closes Diag & Quality)
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showShortcuts = !showShortcuts
                                        if (showShortcuts) {
                                            showDiagnostics = false
                                            showQualityDialog = false
                                        }
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (showShortcuts) Color(0xFF059669) else Color(0xFF334155),
                                        contentColor = Color.White
                                    ),
                                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                    modifier = Modifier.height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("⚡", fontSize = 14.sp, color = Color.White)
                                }

                                // Refresh Keyframe
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        WardSessionManager.requestKeyframe()
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF475569), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                    modifier = Modifier.height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("↻", fontSize = 15.sp, color = Color.White)
                                }

                                // Disconnect / Exit
                                Button(
                                    onClick = {
                                        WardSessionManager.disconnect(context)
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFDC2626), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                    modifier = Modifier.height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("✕", fontSize = 14.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                            }
                        }
                    }

                    // Left Vertical Bar (Collapsed Pill handle)
                    AnimatedVisibility(
                        visible = isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier.align(Alignment.CenterStart).padding(start = 4.dp)
                    ) {
                        Card(
                            onClick = {
                                isBarCollapsed = false
                                lastBarInteraction = System.currentTimeMillis()
                            },
                            colors = CardDefaults.cardColors(containerColor = Color(0xCC141824)),
                            shape = RoundedCornerShape(topEnd = 12.dp, bottomEnd = 12.dp),
                            border = BorderStroke(1.dp, Color(0x33FFFFFF))
                        ) {
                            Column(
                                modifier = Modifier.padding(horizontal = 6.dp, vertical = 12.dp),
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.spacedBy(4.dp)
                            ) {
                                Text("▶", fontSize = 12.sp, color = Color.White)
                                Text(
                                    text = "$fps",
                                    color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                    fontSize = 10.sp,
                                    fontWeight = FontWeight.Bold
                                )
                            }
                        }
                    }

                    // Right Edge Scroll Up / Down Buttons (only denoted by signs ▲ and ▼)
                    Column(
                        modifier = Modifier.align(Alignment.CenterEnd).padding(end = 14.dp),
                        verticalArrangement = Arrangement.spacedBy(16.dp),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        // Scroll Up button
                        Button(
                            onClick = { WardSessionManager.sendInput(WardMessage.MouseScroll(0f, -1.0f)) },
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xCC141824), contentColor = Color.White),
                            border = BorderStroke(1.dp, Color(0x44FFFFFF)),
                            shape = RoundedCornerShape(24.dp),
                            modifier = Modifier.size(48.dp),
                            contentPadding = PaddingValues(0.dp),
                            elevation = ButtonDefaults.buttonElevation(6.dp)
                        ) {
                            Text("▲", fontSize = 18.sp, fontWeight = FontWeight.Bold, color = Color.White)
                        }

                        // Scroll Down button
                        Button(
                            onClick = { WardSessionManager.sendInput(WardMessage.MouseScroll(0f, 1.0f)) },
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xCC141824), contentColor = Color.White),
                            border = BorderStroke(1.dp, Color(0x44FFFFFF)),
                            shape = RoundedCornerShape(24.dp),
                            modifier = Modifier.size(48.dp),
                            contentPadding = PaddingValues(0.dp),
                            elevation = ButtonDefaults.buttonElevation(6.dp)
                        ) {
                            Text("▼", fontSize = 18.sp, fontWeight = FontWeight.Bold, color = Color.White)
                        }
                    }

                } else {
                    // -------------------------------------------------------------
                    // PORTRAIT MODE: Leave horizontal top bar as it is
                    // -------------------------------------------------------------

                    // Top Floating Bar (Expanded in Portrait)
                    AnimatedVisibility(
                        visible = !isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier
                            .align(Alignment.TopCenter)
                            .padding(top = 8.dp, start = 8.dp, end = 8.dp)
                            .fillMaxWidth()
                    ) {
                        Card(
                            modifier = Modifier.fillMaxWidth(),
                            colors = CardDefaults.cardColors(containerColor = Color(0xF0141824)),
                            shape = RoundedCornerShape(16.dp),
                            border = BorderStroke(1.dp, Color(0x33FFFFFF)),
                            elevation = CardDefaults.cardElevation(8.dp)
                        ) {
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 8.dp, vertical = 6.dp),
                                verticalArrangement = Arrangement.spacedBy(6.dp)
                            ) {
                                // Row 1: Connection info, Diag, Keyboard, and Collapse button
                                Row(
                                    modifier = Modifier.horizontalScroll(rememberScrollState()),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                                ) {
                                    Surface(
                                        color = if (sessionStats?.connectionType == TransportType.VPN) Color(0xFF0284C7) else Color(0xFF059669),
                                        shape = RoundedCornerShape(4.dp)
                                    ) {
                                        Text(
                                            text = "v1.2.3 • " + (sessionStats?.connectionType?.displayName ?: "LAN"),
                                            color = Color.White,
                                            fontSize = 9.sp,
                                            fontWeight = FontWeight.Bold,
                                            modifier = Modifier.padding(horizontal = 5.dp, vertical = 2.dp)
                                        )
                                    }

                                    // Clickable Stream Resolution & FPS badge
                                    Surface(
                                        color = Color(0xFF1E293B),
                                        shape = RoundedCornerShape(6.dp),
                                        modifier = Modifier.clickable {
                                            lastBarInteraction = System.currentTimeMillis()
                                            showQualityDialog = !showQualityDialog
                                            if (showQualityDialog) {
                                                showDiagnostics = false
                                                showShortcuts = false
                                            }
                                        }
                                    ) {
                                        Text(
                                            text = "${streamWidth}x${streamHeight} • $fps FPS",
                                            color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                            fontSize = 10.sp,
                                            fontWeight = FontWeight.Bold,
                                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 3.dp)
                                        )
                                    }

                                    if (sessionStats != null && sessionStats!!.latencyMs > 0) {
                                        Text(
                                            text = "${sessionStats!!.latencyMs}ms",
                                            color = Color(0xFF38BDF8),
                                            fontSize = 11.sp,
                                            fontWeight = FontWeight.SemiBold
                                        )
                                    }

                                    // Diagnostics Button (Closes Keys & Quality)
                                    Button(
                                        onClick = {
                                            lastBarInteraction = System.currentTimeMillis()
                                            showDiagnostics = !showDiagnostics
                                            if (showDiagnostics) {
                                                showShortcuts = false
                                                showQualityDialog = false
                                            }
                                        },
                                        colors = ButtonDefaults.buttonColors(
                                            containerColor = if (showDiagnostics) Color(0xFF0284C7) else Color(0xFF334155),
                                            contentColor = Color.White
                                        ),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("📊 Diag", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }

                                    // Quality Button (1080p/720p & 60/30 FPS)
                                    Button(
                                        onClick = {
                                            lastBarInteraction = System.currentTimeMillis()
                                            showQualityDialog = !showQualityDialog
                                            if (showQualityDialog) {
                                                showDiagnostics = false
                                                showShortcuts = false
                                            }
                                        },
                                        colors = ButtonDefaults.buttonColors(
                                            containerColor = if (showQualityDialog) Color(0xFF6366F1) else Color(0xFF334155),
                                            contentColor = Color.White
                                        ),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("⚙️ Quality", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }

                                    // Direct Phone Keyboard Trigger
                                    Button(
                                        onClick = {
                                            lastBarInteraction = System.currentTimeMillis()
                                            showKeyboard()
                                        },
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7), contentColor = Color.White),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("⌨ Type", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }

                                    // Manual collapse button in portrait
                                    Button(
                                        onClick = { isBarCollapsed = true },
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF1E293B), contentColor = Color.White),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("▲", fontSize = 11.sp, color = Color.White)
                                    }
                                }

                                // Row 2: Action Buttons (Shortcuts, Refresh, Disconnect) - Always visible in portrait!
                                Row(
                                    modifier = Modifier.horizontalScroll(rememberScrollState()),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                                ) {
                                    // Quick Shortcuts Bar Toggle (Closes Diag & Quality)
                                    Button(
                                        onClick = {
                                            lastBarInteraction = System.currentTimeMillis()
                                            showShortcuts = !showShortcuts
                                            if (showShortcuts) {
                                                showDiagnostics = false
                                                showQualityDialog = false
                                            }
                                        },
                                        colors = ButtonDefaults.buttonColors(
                                            containerColor = if (showShortcuts) Color(0xFF059669) else Color(0xFF334155),
                                            contentColor = Color.White
                                        ),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("⚡ Keys", fontSize = 11.sp, fontWeight = FontWeight.Medium, color = Color.White)
                                    }

                                    // Refresh Stream / Request Keyframe
                                    Button(
                                        onClick = {
                                            lastBarInteraction = System.currentTimeMillis()
                                            WardSessionManager.requestKeyframe()
                                        },
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF475569), contentColor = Color.White),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("↻ Refresh", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }

                                    // Disconnect Session
                                    Button(
                                        onClick = {
                                            WardSessionManager.disconnect(context)
                                        },
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFDC2626), contentColor = Color.White),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 3.dp),
                                        modifier = Modifier.height(30.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text("✕ Disconnect", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }
                                }
                            }
                        }
                    }

                    // Top Floating Bar (Collapsed Pill in Portrait)
                    AnimatedVisibility(
                        visible = isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier.align(Alignment.TopCenter).padding(top = 8.dp)
                    ) {
                        Card(
                            onClick = {
                                isBarCollapsed = false
                                lastBarInteraction = System.currentTimeMillis()
                            },
                            colors = CardDefaults.cardColors(containerColor = Color(0xCC1E232E)),
                            shape = RoundedCornerShape(12.dp),
                            border = BorderStroke(1.dp, Color(0x33FFFFFF))
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Surface(
                                    color = if (sessionStats?.connectionType == TransportType.VPN) Color(0xFF0284C7) else Color(0xFF059669),
                                    shape = RoundedCornerShape(4.dp)
                                ) {
                                    Text(
                                        text = sessionStats?.connectionType?.displayName ?: "LAN",
                                        color = Color.White,
                                        fontSize = 9.sp,
                                        fontWeight = FontWeight.Bold,
                                        modifier = Modifier.padding(horizontal = 4.dp, vertical = 1.dp)
                                    )
                                }
                                Text(text = "${streamWidth}x${streamHeight}", color = Color.White, fontSize = 11.sp)
                                Text(
                                    text = "$fps FPS",
                                    color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                    fontSize = 11.sp,
                                    fontWeight = FontWeight.Bold
                                )
                                if (sessionStats != null && sessionStats!!.latencyMs > 0) {
                                    Text(
                                        text = "${sessionStats!!.latencyMs}ms",
                                        color = Color(0xFF38BDF8),
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.SemiBold
                                    )
                                }
                                Text("▼", color = Color.White, fontSize = 10.sp)
                            }
                        }
                    }
                }

                // 5. Shortcut & Navigation Toolbar
                AnimatedVisibility(
                    visible = showShortcuts,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = if (isLandscape) {
                        Modifier.align(Alignment.BottomCenter).padding(bottom = 8.dp, start = 64.dp, end = 72.dp)
                    } else {
                        Modifier.align(Alignment.BottomCenter).padding(bottom = 16.dp, start = 8.dp, end = 8.dp)
                    }
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xF2141822)),
                        shape = RoundedCornerShape(16.dp),
                        elevation = CardDefaults.cardElevation(10.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Column(modifier = Modifier.padding(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            // Row 1: Dedicated Scroll and Page Navigation
                            Row(
                                modifier = Modifier.horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(6.dp)
                            ) {
                                val navButton = @Composable { label: String, bgColor: Color, onClick: () -> Unit ->
                                    Button(
                                        onClick = onClick,
                                        colors = ButtonDefaults.buttonColors(
                                            containerColor = bgColor,
                                            contentColor = Color.White
                                        ),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                                        modifier = Modifier.height(34.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text(label, fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                    }
                                }

                                // Dedicated Scroll Up / Down ticks
                                navButton("▲ Scroll Up", Color(0xFF0D9488)) {
                                    WardSessionManager.sendInput(WardMessage.MouseScroll(0f, -1.0f))
                                }
                                navButton("▼ Scroll Down", Color(0xFF0D9488)) {
                                    WardSessionManager.sendInput(WardMessage.MouseScroll(0f, 1.0f))
                                }
                                navButton("PgUp", Color(0xFF334155)) { sendKeysym(KEYSYM_PAGE_UP) }
                                navButton("PgDn", Color(0xFF334155)) { sendKeysym(KEYSYM_PAGE_DOWN) }
                                navButton("Super (Win)", Color(0xFFEA580C)) { sendKeysym(KEYSYM_SUPER) }
                                navButton("Esc", Color(0xFF475569)) { sendKeysym(KEYSYM_ESCAPE) }
                                navButton("Tab", Color(0xFF475569)) { sendKeysym(KEYSYM_TAB) }
                                navButton("Enter", Color(0xFF2563EB)) { sendKeysym(KEYSYM_RETURN) }
                                navButton("Bksp", Color(0xFFDC2626)) { sendKeysym(KEYSYM_BACKSPACE) }
                                navButton("✕ Close", Color(0xFF475569)) { showShortcuts = false }
                            }

                            // Row 2: Clipboard & Arrow navigation
                            Row(
                                modifier = Modifier.horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(6.dp)
                            ) {
                                val actionButton = @Composable { label: String, onClick: () -> Unit ->
                                    Button(
                                        onClick = onClick,
                                        colors = ButtonDefaults.buttonColors(
                                            containerColor = Color(0xFF242A38),
                                            contentColor = Color.White
                                        ),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                                        modifier = Modifier.height(32.dp),
                                        shape = RoundedCornerShape(8.dp)
                                    ) {
                                        Text(label, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Color.White)
                                    }
                                }

                                actionButton("Ctrl+C") { sendCtrlKey(EVDEV_KEY_C) }
                                actionButton("Ctrl+V") { sendCtrlKey(EVDEV_KEY_V) }
                                actionButton("Ctrl+Z") { sendCtrlKey(EVDEV_KEY_Z) }
                                actionButton("Ctrl+A") { sendCtrlKey(EVDEV_KEY_A) }

                                actionButton("← Left") { sendKeysym(KEYSYM_LEFT) }
                                actionButton("↑ Up") { sendKeysym(KEYSYM_UP) }
                                actionButton("↓ Down") { sendKeysym(KEYSYM_DOWN) }
                                actionButton("→ Right") { sendKeysym(KEYSYM_RIGHT) }
                            }
                        }
                    }
                }

                // 6. Diagnostics HUD Card Overlay (Independent floating HUD)
                AnimatedVisibility(
                    visible = showDiagnostics,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier
                        .align(if (isLandscape) Alignment.TopStart else Alignment.TopCenter)
                        .padding(top = if (isLandscape) 16.dp else 70.dp, start = if (isLandscape) 64.dp else 16.dp, end = 16.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xF20B1329)),
                        shape = RoundedCornerShape(14.dp),
                        border = BorderStroke(1.dp, Color(0x6638BDF8)),
                        elevation = CardDefaults.cardElevation(12.dp),
                        modifier = Modifier.width(310.dp)
                    ) {
                        Column(
                            modifier = Modifier
                                .padding(14.dp)
                                .verticalScroll(rememberScrollState())
                                .heightIn(max = if (isLandscape) 280.dp else 520.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "📡 DIAGNOSTICS & QUALITY",
                                    color = Color(0xFF38BDF8),
                                    fontSize = 11.sp,
                                    fontWeight = FontWeight.Bold
                                )
                                IconButton(
                                    onClick = { showDiagnostics = false },
                                    modifier = Modifier.size(22.dp)
                                ) {
                                    Text("✕", color = Color(0xFF94A3B8), fontSize = 13.sp, fontWeight = FontWeight.Bold)
                                }
                            }
                            HorizontalDivider(color = Color(0x3338BDF8), thickness = 1.dp)

                            // STREAM RESOLUTION CONTROLS (PROMINENT AT TOP)
                            Text(
                                text = "🎛 STREAM RESOLUTION",
                                color = Color(0xFF38BDF8),
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold
                            )
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredResolution(1920, 1080)
                                        WardSessionManager.requestStreamConfig(1920, 1080, targetFps)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (streamWidth >= 1920) Color(0xFF0284C7) else Color(0xFF1E293B)
                                    ),
                                    contentPadding = PaddingValues(horizontal = 4.dp, vertical = 2.dp),
                                    modifier = Modifier.weight(1f).height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(
                                        text = if (streamWidth >= 1920) "✓ 1080p FHD" else "1080p FHD",
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.Bold,
                                        color = Color.White
                                    )
                                }
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredResolution(1280, 720)
                                        WardSessionManager.requestStreamConfig(1280, 720, targetFps)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (streamWidth < 1920) Color(0xFF0284C7) else Color(0xFF1E293B)
                                    ),
                                    contentPadding = PaddingValues(horizontal = 4.dp, vertical = 2.dp),
                                    modifier = Modifier.weight(1f).height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(
                                        text = if (streamWidth < 1920) "✓ 720p HD" else "720p HD",
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.Bold,
                                        color = Color.White
                                    )
                                }
                            }

                            // STREAM FRAMERATE CONTROLS
                            Text(
                                text = "⚡ TARGET FRAMERATE",
                                color = Color(0xFF38BDF8),
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold
                            )
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredFps(60)
                                        WardSessionManager.requestStreamConfig(streamWidth, streamHeight, 60)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (targetFps >= 60) Color(0xFF059669) else Color(0xFF1E293B)
                                    ),
                                    contentPadding = PaddingValues(horizontal = 4.dp, vertical = 2.dp),
                                    modifier = Modifier.weight(1f).height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(
                                        text = if (targetFps >= 60) "✓ 60 FPS" else "60 FPS",
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.Bold,
                                        color = Color.White
                                    )
                                }
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredFps(30)
                                        WardSessionManager.requestStreamConfig(streamWidth, streamHeight, 30)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (targetFps < 60) Color(0xFF059669) else Color(0xFF1E293B)
                                    ),
                                    contentPadding = PaddingValues(horizontal = 4.dp, vertical = 2.dp),
                                    modifier = Modifier.weight(1f).height(34.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(
                                        text = if (targetFps < 60) "✓ 30 FPS" else "30 FPS",
                                        fontSize = 11.sp,
                                        fontWeight = FontWeight.Bold,
                                        color = Color.White
                                    )
                                }
                            }

                            HorizontalDivider(color = Color(0x3338BDF8), thickness = 1.dp)

                            Text(
                                text = "📊 LIVE TELEMETRY",
                                color = Color(0xFF94A3B8),
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold
                            )

                            DiagnosticRow("Connection:", sessionStats?.connectionType?.displayName ?: "LAN", Color(0xFF34D399))
                            DiagnosticRow(
                                "Host address:",
                                sessionStats?.remoteAddressMasked ?: maskIpAddress(host, portText.toIntOrNull() ?: 9740),
                                Color.White
                            )
                            DiagnosticRow(
                                "Latency:",
                                "${sessionStats?.latencyMs ?: 0} ms",
                                if ((sessionStats?.latencyMs ?: 0) < 50) Color(0xFF10B981) else Color(0xFFF59E0B)
                            )
                            DiagnosticRow("Packet loss:", "${sessionStats?.packetLossPercent ?: 0}%", Color(0xFF10B981))
                            DiagnosticRow("Stream FPS:", "$fps FPS", if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B))
                            val bitrateMbps = (sessionStats?.bitrateKbps ?: 0) / 1000f
                            DiagnosticRow("Bitrate:", String.format("%.2f Mbps", bitrateMbps), Color.White)
                            DiagnosticRow("Resolution:", "${streamWidth}x${streamHeight}", Color(0xFF38BDF8))
                        }
                    }
                }

                // 7. Dedicated Stream Quality Overlay (Independent floating Card, no OS Dialog window)
                AnimatedVisibility(
                    visible = showQualityDialog,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier
                        .align(Alignment.Center)
                        .padding(16.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xF20B1329)),
                        shape = RoundedCornerShape(16.dp),
                        border = BorderStroke(1.dp, Color(0x66818CF8)),
                        elevation = CardDefaults.cardElevation(12.dp),
                        modifier = Modifier.width(320.dp)
                    ) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(14.dp)
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "🎛 Stream Quality & Resolution",
                                    fontSize = 14.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = Color(0xFF818CF8)
                                )
                                IconButton(
                                    onClick = { showQualityDialog = false },
                                    modifier = Modifier.size(24.dp)
                                ) {
                                    Text("✕", color = Color(0xFF94A3B8), fontSize = 14.sp, fontWeight = FontWeight.Bold)
                                }
                            }
                            HorizontalDivider(color = Color(0x33818CF8), thickness = 1.dp)

                            Text("Desktop Resolution:", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Color.White)
                            Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredResolution(1920, 1080)
                                        WardSessionManager.requestStreamConfig(1920, 1080, targetFps)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (streamWidth >= 1920) Color(0xFF0284C7) else Color(0xFF1E293B)
                                    ),
                                    modifier = Modifier.weight(1f).height(40.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(if (streamWidth >= 1920) "✓ 1080p FHD" else "1080p FHD", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredResolution(1280, 720)
                                        WardSessionManager.requestStreamConfig(1280, 720, targetFps)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (streamWidth < 1920) Color(0xFF0284C7) else Color(0xFF1E293B)
                                    ),
                                    modifier = Modifier.weight(1f).height(40.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(if (streamWidth < 1920) "✓ 720p HD" else "720p HD", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                            }

                            Text("Stream Target Framerate:", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Color.White)
                            Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredFps(60)
                                        WardSessionManager.requestStreamConfig(streamWidth, streamHeight, 60)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (targetFps >= 60) Color(0xFF059669) else Color(0xFF1E293B)
                                    ),
                                    modifier = Modifier.weight(1f).height(40.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(if (targetFps >= 60) "✓ 60 FPS" else "60 FPS", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                                Button(
                                    onClick = {
                                        deviceStore.setPreferredFps(30)
                                        WardSessionManager.requestStreamConfig(streamWidth, streamHeight, 30)
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (targetFps < 60) Color(0xFF059669) else Color(0xFF1E293B)
                                    ),
                                    modifier = Modifier.weight(1f).height(40.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(if (targetFps < 60) "✓ 30 FPS" else "30 FPS", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                            }

                            Button(
                                onClick = { showQualityDialog = false },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7)),
                                modifier = Modifier.fillMaxWidth().height(38.dp),
                                shape = RoundedCornerShape(8.dp)
                            ) {
                                Text("Done", color = Color.White, fontWeight = FontWeight.Bold, fontSize = 12.sp)
                            }
                        }
                    }
                }
            }
            else -> {
                // -------------------------------------------------------------
                // DISCONNECTED: Device Identity Screen (AJAY-UBUNTU Paired Card)
                // -------------------------------------------------------------
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    // WARD Header
                    Text(
                        text = "WARD",
                        fontSize = 28.sp,
                        fontWeight = FontWeight.ExtraBold,
                        color = Color.White,
                        letterSpacing = 2.sp
                    )
                    Spacer(modifier = Modifier.height(2.dp))
                    Text(
                        text = "My Computers",
                        fontSize = 15.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = Color(0xFF38BDF8)
                    )
                    Spacer(modifier = Modifier.height(18.dp))

                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xFF141824)),
                        shape = RoundedCornerShape(24.dp),
                        border = BorderStroke(1.dp, Color(0x3338BDF8)),
                        elevation = CardDefaults.cardElevation(12.dp),
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp)
                    ) {
                        Column(
                            modifier = Modifier.padding(24.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            // Device Icon / Avatar
                            Box(
                                modifier = Modifier
                                    .size(72.dp)
                                    .background(Color(0x260284C7), shape = RoundedCornerShape(20.dp)),
                                contentAlignment = Alignment.Center
                            ) {
                                Text("🖥", fontSize = 36.sp)
                            }

                            // Device Identity Title & Subtitle
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Text(
                                    text = pairedDevice.deviceName,
                                    fontSize = 24.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = Color.White
                                )
                                Spacer(modifier = Modifier.height(2.dp))
                                Text(
                                    text = "Wayland Host • Unattended Remote Desktop",
                                    fontSize = 12.sp,
                                    color = Color(0xFF94A3B8)
                                )
                            }

                            // Status Badges Row (Paired + Online/Offline)
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(10.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                // Paired Badge
                                Surface(
                                    shape = RoundedCornerShape(50),
                                    color = Color(0x2214B8A6),
                                    border = BorderStroke(1.dp, Color(0x6614B8A6))
                                ) {
                                    Row(
                                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 5.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(4.dp)
                                    ) {
                                        Text("✓", fontSize = 11.sp, color = Color(0xFF2DD4BF), fontWeight = FontWeight.Bold)
                                        Text("Paired", fontSize = 11.sp, color = Color(0xFF2DD4BF), fontWeight = FontWeight.SemiBold)
                                    }
                                }

                                // Online / Offline Badge
                                val statusText = when (isOnline) {
                                    true -> "● Online"
                                    false -> "○ Offline"
                                    null -> "◌ Probing..."
                                }
                                val statusBg = when (isOnline) {
                                    true -> Color(0x2210B981)
                                    false -> Color(0x22EF4444)
                                    null -> Color(0x2264748B)
                                }
                                val statusBorder = when (isOnline) {
                                    true -> Color(0x6610B981)
                                    false -> Color(0x66EF4444)
                                    null -> Color(0x6664748B)
                                }
                                val statusColor = when (isOnline) {
                                    true -> Color(0xFF34D399)
                                    false -> Color(0xFFF87171)
                                    null -> Color(0xFF94A3B8)
                                }

                                Surface(
                                    shape = RoundedCornerShape(50),
                                    color = statusBg,
                                    border = BorderStroke(1.dp, statusBorder)
                                ) {
                                    Text(
                                        text = statusText,
                                        fontSize = 11.sp,
                                        color = statusColor,
                                        fontWeight = FontWeight.SemiBold,
                                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 5.dp)
                                    )
                                }
                            }

                            // Host Metadata Snippet
                            Surface(
                                shape = RoundedCornerShape(12.dp),
                                color = Color(0xFF1E2433),
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween
                                    ) {
                                        Text("Last Known IP", fontSize = 11.sp, color = Color(0xFF64748B))
                                        Text("$host:$portText", fontSize = 11.sp, color = Color(0xFFCBD5E1), fontWeight = FontWeight.Medium)
                                    }
                                    if (pairedDevice.deviceId.isNotBlank()) {
                                        Row(
                                            modifier = Modifier.fillMaxWidth(),
                                            horizontalArrangement = Arrangement.SpaceBetween
                                        ) {
                                            Text("Device ID", fontSize = 11.sp, color = Color(0xFF64748B))
                                            Text(
                                                pairedDevice.deviceId.take(16) + "...",
                                                fontSize = 11.sp,
                                                color = Color(0xFF38BDF8),
                                                fontWeight = FontWeight.Medium
                                            )
                                        }
                                    }
                                }
                            }

                            if (state is ConnectionState.Failed) {
                                Surface(
                                    shape = RoundedCornerShape(10.dp),
                                    color = Color(0x26EF4444),
                                    border = BorderStroke(1.dp, Color(0x66EF4444)),
                                    modifier = Modifier.fillMaxWidth()
                                ) {
                                    Text(
                                        text = "Connection Failed: ${state.error}",
                                        color = Color(0xFFFCA5A5),
                                        fontSize = 12.sp,
                                        modifier = Modifier.padding(10.dp)
                                    )
                                }
                            }

                            // Primary Connect Button
                            Button(
                                onClick = {
                                    val port = portText.toIntOrNull() ?: 9740
                                    WardSessionManager.connect(
                                        context = context,
                                        host = host.trim(),
                                        port = port,
                                        pairedDevice = pairedDevice,
                                        deviceStore = deviceStore,
                                        onPinRequired = { callback ->
                                            pendingPinCallback = callback
                                        },
                                        onDeviceUpdated = { updated ->
                                            pairedDevice = updated
                                        }
                                    )
                                },
                                enabled = state !is ConnectionState.Connecting && state !is ConnectionState.Reconnecting,
                                modifier = Modifier.fillMaxWidth().height(52.dp),
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7)),
                                shape = RoundedCornerShape(14.dp)
                            ) {
                                if (state is ConnectionState.Connecting) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.size(22.dp),
                                        color = Color.White,
                                        strokeWidth = 2.dp
                                    )
                                    Spacer(modifier = Modifier.width(12.dp))
                                    Text("Connecting to ${pairedDevice.deviceName}...", fontSize = 14.sp, color = Color.White)
                                } else if (state is ConnectionState.Reconnecting) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.size(22.dp),
                                        color = Color(0xFFFDE68A),
                                        strokeWidth = 2.dp
                                    )
                                    Spacer(modifier = Modifier.width(12.dp))
                                    Text("Reconnecting (#${state.attempt})...", fontSize = 14.sp, color = Color(0xFFFDE68A))
                                } else {
                                    Text("CONNECT", fontSize = 16.sp, fontWeight = FontWeight.ExtraBold, letterSpacing = 1.sp, color = Color.White)
                                }
                            }

                            if (state is ConnectionState.Reconnecting) {
                                Spacer(modifier = Modifier.height(4.dp))
                                TextButton(
                                    onClick = { WardSessionManager.disconnect(context) }
                                ) {
                                    Text("Cancel Reconnect", color = Color(0xFFF87171), fontSize = 12.sp)
                                }
                            }

                            // Collapsible Settings for Manual IP / Port (Developer Options)
                            TextButton(
                                onClick = { showManualSettings = !showManualSettings }
                            ) {
                                Text(
                                    text = if (showManualSettings) "▲ Hide Developer Settings" else "▼ Developer Options (IP & Port)",
                                    color = Color(0xFF94A3B8),
                                    fontSize = 12.sp
                                )
                            }

                            AnimatedVisibility(visible = showManualSettings) {
                                Column(verticalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
                                    OutlinedTextField(
                                        value = host,
                                        onValueChange = {
                                            host = it
                                            val port = portText.toIntOrNull() ?: 9740
                                            deviceStore.savePairedDevice(pairedDevice.copy(lastKnownHost = it.trim(), lastKnownPort = port))
                                        },
                                        label = { Text("Server Host / IP") },
                                        singleLine = true,
                                        modifier = Modifier.fillMaxWidth(),
                                        colors = OutlinedTextFieldDefaults.colors(
                                            focusedBorderColor = Color(0xFF38BDF8),
                                            unfocusedBorderColor = Color(0xFF475569),
                                            focusedTextColor = Color.White,
                                            unfocusedTextColor = Color.White
                                        )
                                    )

                                    OutlinedTextField(
                                        value = portText,
                                        onValueChange = {
                                            portText = it
                                            val port = it.toIntOrNull() ?: 9740
                                            deviceStore.savePairedDevice(pairedDevice.copy(lastKnownHost = host.trim(), lastKnownPort = port))
                                        },
                                        label = { Text("Port (Default: 9740)") },
                                        singleLine = true,
                                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                                        modifier = Modifier.fillMaxWidth(),
                                        colors = OutlinedTextFieldDefaults.colors(
                                            focusedBorderColor = Color(0xFF38BDF8),
                                            unfocusedBorderColor = Color(0xFF475569),
                                            focusedTextColor = Color.White,
                                            unfocusedTextColor = Color.White
                                        )
                                    )

                                    HorizontalDivider(color = Color(0x3338BDF8), thickness = 1.dp)

                                    Text(
                                        text = "Stream Quality Preference",
                                        color = Color(0xFF38BDF8),
                                        fontSize = 12.sp,
                                        fontWeight = FontWeight.SemiBold
                                    )

                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically
                                    ) {
                                        Text("Default Resolution:", color = Color(0xFF94A3B8), fontSize = 12.sp)
                                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                            FilterChip(
                                                selected = selectedResWidth >= 1920,
                                                onClick = {
                                                    selectedResWidth = 1920
                                                    selectedResHeight = 1080
                                                    deviceStore.setPreferredResolution(1920, 1080)
                                                },
                                                label = { Text("1080p", fontSize = 11.sp) }
                                            )
                                            FilterChip(
                                                selected = selectedResWidth < 1920,
                                                onClick = {
                                                    selectedResWidth = 1280
                                                    selectedResHeight = 720
                                                    deviceStore.setPreferredResolution(1280, 720)
                                                },
                                                label = { Text("720p", fontSize = 11.sp) }
                                            )
                                        }
                                    }

                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically
                                    ) {
                                        Text("Default Framerate:", color = Color(0xFF94A3B8), fontSize = 12.sp)
                                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                            FilterChip(
                                                selected = selectedFps >= 60,
                                                onClick = {
                                                    selectedFps = 60
                                                    deviceStore.setPreferredFps(60)
                                                },
                                                label = { Text("60 FPS", fontSize = 11.sp) }
                                            )
                                            FilterChip(
                                                selected = selectedFps < 60,
                                                onClick = {
                                                    selectedFps = 30
                                                    deviceStore.setPreferredFps(30)
                                                },
                                                label = { Text("30 FPS", fontSize = 11.sp) }
                                            )
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if (pendingPinCallback != null) {
        AlertDialog(
            onDismissRequest = {
                val cb = pendingPinCallback
                pendingPinCallback = null
                cb?.invoke("")
            },
            containerColor = Color(0xFF141824),
            shape = RoundedCornerShape(20.dp),
            title = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("🔐", fontSize = 20.sp)
                    Text("Permanent Host PIN", color = Color.White, fontWeight = FontWeight.Bold, fontSize = 18.sp)
                }
            },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(
                        "Enter the permanent PIN configured for '${pairedDevice.deviceName}':",
                        color = Color(0xFFCBD5E1),
                        fontSize = 13.sp
                    )
                    OutlinedTextField(
                        value = enteredPinText,
                        onValueChange = {
                            if (it.length <= 6 && it.all { c -> c.isDigit() }) {
                                enteredPinText = it
                            }
                        },
                        label = { Text("6-Digit PIN") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedBorderColor = Color(0xFF38BDF8),
                            unfocusedBorderColor = Color(0xFF475569),
                            focusedTextColor = Color.White,
                            unfocusedTextColor = Color.White
                        )
                    )
                }
            },
            confirmButton = {
                Button(
                    onClick = {
                        val pin = enteredPinText
                        enteredPinText = ""
                        val cb = pendingPinCallback
                        pendingPinCallback = null
                        cb?.invoke(pin)
                    },
                    enabled = enteredPinText.length == 6,
                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7)),
                    shape = RoundedCornerShape(10.dp)
                ) {
                    Text("Authenticate & Connect", color = Color.White, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = {
                        enteredPinText = ""
                        val cb = pendingPinCallback
                        pendingPinCallback = null
                        cb?.invoke("")
                    }
                ) {
                    Text("Cancel", color = Color(0xFF94A3B8))
                }
            }
        )
    }
}

@Composable
private fun DiagnosticRow(label: String, value: String, valueColor: Color = Color.White) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(text = label, color = Color(0xFF94A3B8), fontSize = 12.sp)
        Text(text = value, color = valueColor, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
    }
}

