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
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.ui.platform.LocalContext
import com.example.ward.data.PairedDevice
import com.example.ward.data.PairedDeviceStore
import com.example.ward.decoder.VideoDecoder
import com.example.ward.network.ConnectionState
import com.example.ward.network.WardClient
import com.example.ward.protocol.WardMessage
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

@Composable
fun WardRemoteApp() {
    val context = LocalContext.current
    val deviceStore = remember { PairedDeviceStore(context) }
    var pairedDevice by remember { mutableStateOf(deviceStore.getPairedDevice()) }

    var host by remember { mutableStateOf(pairedDevice.lastKnownHost) }
    var portText by remember { mutableStateOf(pairedDevice.lastKnownPort.toString()) }
    var showManualSettings by remember { mutableStateOf(false) }
    var isOnline by remember { mutableStateOf<Boolean?>(null) }

    var connectionState by remember { mutableStateOf<ConnectionState>(ConnectionState.Disconnected) }
    var fps by remember { mutableIntStateOf(0) }
    var streamWidth by remember { mutableIntStateOf(1920) }
    var streamHeight by remember { mutableIntStateOf(1080) }
    var showShortcuts by remember { mutableStateOf(false) }

    // Auto-contracting bar state & interaction timer
    var isBarCollapsed by remember { mutableStateOf(false) }
    var lastBarInteraction by remember { mutableLongStateOf(System.currentTimeMillis()) }

    var remoteInputView: RemoteInputView? by remember { mutableStateOf(null) }
    var videoDecoder: VideoDecoder? by remember { mutableStateOf(null) }
    var wardClient: WardClient? by remember { mutableStateOf(null) }

    // Probe host reachability periodically when disconnected
    LaunchedEffect(host, portText, connectionState) {
        if (connectionState !is ConnectionState.Connected) {
            val targetHost = host.trim()
            val targetPort = portText.toIntOrNull() ?: 9740
            while (true) {
                val reachable = withContext(Dispatchers.IO) {
                    try {
                        Socket().use { s ->
                            s.connect(InetSocketAddress(targetHost, targetPort), 1200)
                            true
                        }
                    } catch (e: Exception) {
                        false
                    }
                }
                isOnline = reachable
                delay(4000)
            }
        }
    }

    fun sendKeysym(keysym: Int) {
        val code = keysym or KEYSYM_FLAG
        wardClient?.sendKeyPress(code)
    }

    fun sendCtrlKey(evdevKey: Int) {
        wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
        wardClient?.sendInput(WardMessage.KeyDown(evdevKey))
        wardClient?.sendInput(WardMessage.KeyUp(evdevKey))
        wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
    }

    fun showKeyboard() {
        remoteInputView?.let { view ->
            view.requestFocus()
            val imm = view.context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            imm?.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
        }
    }

    // Auto-contract bar after 4 seconds of inactivity
    LaunchedEffect(isBarCollapsed, lastBarInteraction) {
        if (!isBarCollapsed) {
            delay(4000)
            isBarCollapsed = true
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            wardClient?.disconnect()
            videoDecoder?.stop()
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
                                        val decoder = VideoDecoder(holder.surface) { newFps ->
                                            fps = newFps
                                        }
                                        videoDecoder = decoder
                                        if (streamWidth > 0 && streamHeight > 0) {
                                            decoder.start(streamWidth, streamHeight)
                                        }
                                        wardClient?.requestKeyframe()
                                    }

                                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

                                    override fun surfaceDestroyed(holder: SurfaceHolder) {
                                        videoDecoder?.stop()
                                        videoDecoder = null
                                    }
                                })
                            }
                        }
                    )
                }

                // 2. Full-screen Multi-Gesture Touchpad Overlay (handles video + blank area below)
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

                            setOnTouchListener { v, event ->
                                pointerCount = event.pointerCount

                                when (event.actionMasked) {
                                    MotionEvent.ACTION_DOWN -> {
                                        downX = event.x
                                        downY = event.y
                                        lastX1 = event.x
                                        lastY1 = event.y
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
                                            val totalDist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                                            // Enter drag mode if held down for > 350ms
                                            if (!isDragging && now - downTime > 350 && totalDist < 25) {
                                                isDragging = true
                                                // Mouse button down for drag & drop / text selection
                                                wardClient?.sendInput(WardMessage.MouseButton(1, 1))
                                            }

                                            wardClient?.sendInput(WardMessage.MouseMove(dx, dy))
                                            lastX1 = event.x
                                            lastY1 = event.y
                                        } else if (pointerCount == 2) {
                                            // 2-finger scroll
                                            val currentY1 = event.getY(0)
                                            val currentY2 = event.getY(1)
                                            val currentX1 = event.getX(0)
                                            val currentX2 = event.getX(1)

                                            val dy = ((currentY1 - lastY1) + (currentY2 - lastY2)) / 2f
                                            val dx = ((currentX1 - lastX1) + (currentX2 - lastX2)) / 2f

                                            if (Math.abs(dy) > 2f) {
                                                // Positive dy (finger down) -> scroll down (-1)
                                                // Negative dy (finger up) -> scroll up (1)
                                                val stepY = if (dy > 0) 1.0f else -1.0f
                                                wardClient?.sendInput(WardMessage.MouseScroll(0f, stepY))
                                            }

                                            if (Math.abs(dx) > 4f) {
                                                val stepX = if (dx > 0) 1.0f else -1.0f
                                                wardClient?.sendInput(WardMessage.MouseScroll(stepX, 0f))
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
                                                threeFingerSwipeStartY = currentY // reset to prevent retrigger
                                            }
                                        }
                                        true
                                    }

                                    MotionEvent.ACTION_POINTER_UP -> {
                                        val duration = System.currentTimeMillis() - downTime
                                        if (pointerCount == 2 && duration < 250) {
                                            // 2-finger tap -> Right Click!
                                            wardClient?.sendInput(WardMessage.MouseButton(2, 1))
                                            wardClient?.sendInput(WardMessage.MouseButton(2, 0))
                                        } else if (pointerCount == 3 && duration < 250) {
                                            // 3-finger tap -> Middle Click!
                                            wardClient?.sendInput(WardMessage.MouseButton(3, 1))
                                            wardClient?.sendInput(WardMessage.MouseButton(3, 0))
                                        }
                                        true
                                    }

                                    MotionEvent.ACTION_UP -> {
                                        val duration = System.currentTimeMillis() - downTime
                                        val dist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                                        if (isDragging) {
                                            // Release drag
                                            wardClient?.sendInput(WardMessage.MouseButton(1, 0))
                                            isDragging = false
                                        } else if (duration < 220 && dist < 20) {
                                            // 1-finger quick tap -> Left Click
                                            wardClient?.sendInput(WardMessage.MouseButton(1, 1))
                                            wardClient?.sendInput(WardMessage.MouseButton(1, 0))
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

                                // Stats: FPS and Resolution
                                Column(horizontalAlignment = Alignment.CenterHorizontally) {
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

                                // Shortcuts Toggle
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showShortcuts = !showShortcuts
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
                                        wardClient?.requestKeyframe()
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
                                        wardClient?.disconnect()
                                        videoDecoder?.stop()
                                        connectionState = ConnectionState.Disconnected
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
                            onClick = { wardClient?.sendInput(WardMessage.MouseScroll(0f, -1.0f)) },
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
                            onClick = { wardClient?.sendInput(WardMessage.MouseScroll(0f, 1.0f)) },
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

                    // Top Floating Bar (Expanded)
                    AnimatedVisibility(
                        visible = !isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier.align(Alignment.TopCenter).padding(top = 28.dp, start = 12.dp, end = 12.dp)
                    ) {
                        Card(
                            colors = CardDefaults.cardColors(containerColor = Color(0xEE1E232E)),
                            shape = RoundedCornerShape(16.dp),
                            elevation = CardDefaults.cardElevation(8.dp)
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Text(
                                    text = "${streamWidth}x${streamHeight}",
                                    color = Color.White,
                                    fontSize = 12.sp,
                                    fontWeight = FontWeight.Bold
                                )
                                Text(
                                    text = "$fps FPS",
                                    color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                    fontSize = 12.sp,
                                    fontWeight = FontWeight.Bold
                                )

                                // Direct Phone Keyboard Trigger
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showKeyboard()
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0284C7), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                    modifier = Modifier.height(32.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("⌨ Type", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }

                                // Quick Shortcuts Bar Toggle
                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        showShortcuts = !showShortcuts
                                    },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (showShortcuts) Color(0xFF059669) else Color(0xFF334155),
                                        contentColor = Color.White
                                    ),
                                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                    modifier = Modifier.height(32.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("Shortcuts", fontSize = 11.sp, fontWeight = FontWeight.Medium, color = Color.White)
                                }

                                Button(
                                    onClick = {
                                        lastBarInteraction = System.currentTimeMillis()
                                        wardClient?.requestKeyframe()
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF475569), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                    modifier = Modifier.height(32.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("Refresh", fontSize = 11.sp, color = Color.White)
                                }

                                Button(
                                    onClick = {
                                        wardClient?.disconnect()
                                        videoDecoder?.stop()
                                        connectionState = ConnectionState.Disconnected
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFDC2626), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                    modifier = Modifier.height(32.dp),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text("Exit", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }

                                // Manual collapse button in portrait
                                Button(
                                    onClick = { isBarCollapsed = true },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF1E293B), contentColor = Color.White),
                                    contentPadding = PaddingValues(horizontal = 6.dp, vertical = 2.dp),
                                    modifier = Modifier.height(28.dp),
                                    shape = RoundedCornerShape(6.dp)
                                ) {
                                    Text("▲", fontSize = 10.sp, color = Color.White)
                                }
                            }
                        }
                    }

                    // Top Floating Bar (Collapsed Pill in Portrait)
                    AnimatedVisibility(
                        visible = isBarCollapsed,
                        enter = fadeIn(),
                        exit = fadeOut(),
                        modifier = Modifier.align(Alignment.TopCenter).padding(top = 24.dp)
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
                                Text(text = "${streamWidth}x${streamHeight}", color = Color.White, fontSize = 11.sp)
                                Text(
                                    text = "$fps FPS",
                                    color = if (fps >= 25) Color(0xFF10B981) else Color(0xFFF59E0B),
                                    fontSize = 11.sp,
                                    fontWeight = FontWeight.Bold
                                )
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
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
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
                                    wardClient?.sendInput(WardMessage.MouseScroll(0f, -1.0f))
                                }
                                navButton("▼ Scroll Down", Color(0xFF0D9488)) {
                                    wardClient?.sendInput(WardMessage.MouseScroll(0f, 1.0f))
                                }
                                navButton("PgUp", Color(0xFF334155)) { sendKeysym(KEYSYM_PAGE_UP) }
                                navButton("PgDn", Color(0xFF334155)) { sendKeysym(KEYSYM_PAGE_DOWN) }
                                navButton("Super (Win)", Color(0xFFEA580C)) { sendKeysym(KEYSYM_SUPER) }
                                navButton("Esc", Color(0xFF475569)) { sendKeysym(KEYSYM_ESCAPE) }
                                navButton("Tab", Color(0xFF475569)) { sendKeysym(KEYSYM_TAB) }
                                navButton("Enter", Color(0xFF2563EB)) { sendKeysym(KEYSYM_RETURN) }
                                navButton("Bksp", Color(0xFFDC2626)) { sendKeysym(KEYSYM_BACKSPACE) }
                            }

                            // Row 2: Clipboard & Arrow navigation
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
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
                                    text = "Wayland Host • Persistent Identity",
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
                                    val client = WardClient(
                                        onStateChanged = { newState ->
                                            connectionState = newState
                                        },
                                        onVideoConfig = { w, h, _ ->
                                            streamWidth = w
                                            streamHeight = h
                                            videoDecoder?.start(w, h)
                                        },
                                        onVideoFrame = { data, ts, isKeyframe ->
                                            videoDecoder?.feedFrame(data, ts, isKeyframe)
                                        },
                                        onSessionAccepted = { accepted ->
                                            val updated = deviceStore.updateFromSession(accepted, host.trim(), port)
                                            pairedDevice = updated
                                        }
                                    )
                                    wardClient = client
                                    client.connect(host.trim(), port)
                                },
                                enabled = state !is ConnectionState.Connecting,
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
                                } else {
                                    Text("Connect to ${pairedDevice.deviceName}", fontSize = 15.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                }
                            }

                            // Collapsible Settings for Manual IP / Port
                            TextButton(
                                onClick = { showManualSettings = !showManualSettings }
                            ) {
                                Text(
                                    text = if (showManualSettings) "▲ Hide Address Settings" else "▼ Edit Address / Port",
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
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
