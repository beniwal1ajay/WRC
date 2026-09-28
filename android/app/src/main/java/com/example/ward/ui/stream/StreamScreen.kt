package com.example.ward.ui.stream

import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import com.example.ward.decoder.VideoDecoder
import com.example.ward.network.ConnectionState
import com.example.ward.network.WardClient
import com.example.ward.protocol.WardMessage

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

@Composable
fun WardRemoteApp() {
    var host by remember { mutableStateOf("10.31.1.234") }
    var portText by remember { mutableStateOf("9740") }
    var connectionState by remember { mutableStateOf<ConnectionState>(ConnectionState.Disconnected) }
    var fps by remember { mutableIntStateOf(0) }
    var streamWidth by remember { mutableIntStateOf(1920) }
    var streamHeight by remember { mutableIntStateOf(1080) }
    var showControls by remember { mutableStateOf(true) }
    var showShortcuts by remember { mutableStateOf(false) }

    // Direct real-time typing state
    var imeBuffer by remember { mutableStateOf(TextFieldValue("")) }
    val focusRequester = remember { FocusRequester() }
    val keyboardController = LocalSoftwareKeyboardController.current

    var videoDecoder: VideoDecoder? by remember { mutableStateOf(null) }
    var wardClient: WardClient? by remember { mutableStateOf(null) }

    fun sendKeysym(keysym: Int) {
        val code = keysym or KEYSYM_FLAG
        wardClient?.sendInput(WardMessage.KeyDown(code))
        wardClient?.sendInput(WardMessage.KeyUp(code))
    }

    fun sendCtrlKey(evdevKey: Int) {
        wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
        wardClient?.sendInput(WardMessage.KeyDown(evdevKey))
        wardClient?.sendInput(WardMessage.KeyUp(evdevKey))
        wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
    }

    DisposableEffect(Unit) {
        onDispose {
            wardClient?.disconnect()
            videoDecoder?.stop()
        }
    }

    Box(modifier = Modifier.fillMaxSize().background(Color.Black)) {
        when (val state = connectionState) {
            is ConnectionState.Connected -> {
                // 1. Aspect-ratio preserved video surface
                BoxWithConstraints(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    val containerAspect = maxWidth / maxHeight
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

                // 3. Invisible Real-Time Input Connection: Types directly to PC
                BasicTextField(
                    value = imeBuffer,
                    onValueChange = { newText ->
                        val oldStr = imeBuffer.text
                        val newStr = newText.text

                        if (newStr.length > oldStr.length) {
                            // Characters typed directly -> send to PC instantly!
                            val addedChars = newStr.substring(oldStr.length)
                            for (ch in addedChars) {
                                if (ch == '\n') {
                                    sendKeysym(KEYSYM_RETURN)
                                } else {
                                    sendKeysym(ch.code)
                                }
                            }
                        } else if (newStr.length < oldStr.length) {
                            // Backspace tapped on phone keyboard -> send Backspace to PC!
                            val deletedCount = oldStr.length - newStr.length
                            repeat(deletedCount) {
                                sendKeysym(KEYSYM_BACKSPACE)
                            }
                        }

                        // Maintain a safe buffer size so text input never fills up memory
                        imeBuffer = if (newStr.length > 50) {
                            TextFieldValue("")
                        } else {
                            newText
                        }
                    },
                    modifier = Modifier
                        .size(1.dp)
                        .alpha(0.001f)
                        .focusRequester(focusRequester),
                    keyboardOptions = KeyboardOptions(
                        autoCorrectEnabled = false,
                        keyboardType = KeyboardType.Text,
                        imeAction = ImeAction.Default
                    )
                )

                // 4. Top Floating Bar (Stats, Keyboard Toggle, Shortcuts Toggle, Refresh, Exit)
                AnimatedVisibility(
                    visible = showControls,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier.align(Alignment.TopCenter).padding(top = 28.dp, start = 12.dp, end = 12.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xDD1E1E1E)),
                        shape = RoundedCornerShape(16.dp),
                        elevation = CardDefaults.cardElevation(8.dp)
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Text(
                                text = "${streamWidth}x${streamHeight}",
                                color = Color(0xFF90CAF9),
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold
                            )
                            Text(
                                text = "$fps FPS",
                                color = if (fps >= 25) Color(0xFF81C784) else Color(0xFFFFB74D),
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold
                            )

                            // Direct Phone Keyboard Trigger
                            Button(
                                onClick = {
                                    focusRequester.requestFocus()
                                    keyboardController?.show()
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0288D1)),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                modifier = Modifier.height(30.dp)
                            ) {
                                Text("⌨ Type", fontSize = 11.sp, fontWeight = FontWeight.Bold)
                            }

                            // Quick Shortcuts Bar Toggle
                            Button(
                                onClick = { showShortcuts = !showShortcuts },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = if (showShortcuts) Color(0xFF00796B) else Color(0xFF37474F)
                                ),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                modifier = Modifier.height(30.dp)
                            ) {
                                Text("Shortcuts", fontSize = 11.sp)
                            }

                            Button(
                                onClick = { wardClient?.requestKeyframe() },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF37474F)),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                modifier = Modifier.height(30.dp)
                            ) {
                                Text("Refresh", fontSize = 11.sp)
                            }

                            Button(
                                onClick = {
                                    wardClient?.disconnect()
                                    videoDecoder?.stop()
                                    connectionState = ConnectionState.Disconnected
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFD32F2F)),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                modifier = Modifier.height(30.dp)
                            ) {
                                Text("Exit", fontSize = 11.sp)
                            }
                        }
                    }
                }

                // 5. Bottom Shortcut & Scroll Toolbar (Sticky overlay with Scroll Up/Down, Page Up/Down, etc.)
                AnimatedVisibility(
                    visible = showShortcuts,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 16.dp, start = 8.dp, end = 8.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xEE1A1A1A)),
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
                                val navButton = @Composable { label: String, color: Color, onClick: () -> Unit ->
                                    Button(
                                        onClick = onClick,
                                        colors = ButtonDefaults.buttonColors(containerColor = color),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                                        modifier = Modifier.height(32.dp)
                                    ) {
                                        Text(label, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                                    }
                                }

                                // Dedicated Scroll Up / Down ticks
                                navButton("▲ Scroll Up", Color(0xFF00695C)) {
                                    wardClient?.sendInput(WardMessage.MouseScroll(0f, -1.0f))
                                }
                                navButton("▼ Scroll Down", Color(0xFF00695C)) {
                                    wardClient?.sendInput(WardMessage.MouseScroll(0f, 1.0f))
                                }
                                navButton("PgUp", Color(0xFF37474F)) { sendKeysym(KEYSYM_PAGE_UP) }
                                navButton("PgDn", Color(0xFF37474F)) { sendKeysym(KEYSYM_PAGE_DOWN) }
                                navButton("Super (Win)", Color(0xFFE65100)) { sendKeysym(KEYSYM_SUPER) }
                                navButton("Esc", Color(0xFF37474F)) { sendKeysym(KEYSYM_ESCAPE) }
                                navButton("Tab", Color(0xFF37474F)) { sendKeysym(KEYSYM_TAB) }
                                navButton("Enter", Color(0xFF0277BD)) { sendKeysym(KEYSYM_RETURN) }
                                navButton("Bksp", Color(0xFFC62828)) { sendKeysym(KEYSYM_BACKSPACE) }
                            }

                            // Row 2: Clipboard & Arrow navigation
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(6.dp)
                            ) {
                                val actionButton = @Composable { label: String, onClick: () -> Unit ->
                                    Button(
                                        onClick = onClick,
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF263238)),
                                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                                        modifier = Modifier.height(30.dp)
                                    ) {
                                        Text(label, fontSize = 11.sp)
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
                // Connection Form Screen
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xFF1E1E1E)),
                        shape = RoundedCornerShape(24.dp),
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp)
                    ) {
                        Column(
                            modifier = Modifier.padding(24.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            Text(
                                text = "WARD Desktop",
                                fontSize = 26.sp,
                                fontWeight = FontWeight.Bold,
                                color = Color.White
                            )
                            Text(
                                text = "Wayland Android Remote Desktop",
                                fontSize = 14.sp,
                                color = Color(0xFFB0BEC5)
                            )

                            Spacer(modifier = Modifier.height(8.dp))

                            OutlinedTextField(
                                value = host,
                                onValueChange = { host = it },
                                label = { Text("Server IP Address") },
                                singleLine = true,
                                modifier = Modifier.fillMaxWidth(),
                                colors = OutlinedTextFieldDefaults.colors(
                                    focusedBorderColor = Color(0xFF4FC3F7),
                                    unfocusedBorderColor = Color(0xFF546E7A),
                                    focusedTextColor = Color.White,
                                    unfocusedTextColor = Color.White
                                )
                            )

                            OutlinedTextField(
                                value = portText,
                                onValueChange = { portText = it },
                                label = { Text("Port (Default: 9740)") },
                                singleLine = true,
                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                                modifier = Modifier.fillMaxWidth(),
                                colors = OutlinedTextFieldDefaults.colors(
                                    focusedBorderColor = Color(0xFF4FC3F7),
                                    unfocusedBorderColor = Color(0xFF546E7A),
                                    focusedTextColor = Color.White,
                                    unfocusedTextColor = Color.White
                                )
                            )

                            if (state is ConnectionState.Failed) {
                                Text(
                                    text = "Connection Failed: ${state.error}",
                                    color = Color(0xFFEF5350),
                                    fontSize = 13.sp
                                )
                            }

                            Spacer(modifier = Modifier.height(8.dp))

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
                                        }
                                    )
                                    wardClient = client
                                    client.connect(host.trim(), port)
                                },
                                enabled = state !is ConnectionState.Connecting,
                                modifier = Modifier.fillMaxWidth().height(50.dp),
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0288D1)),
                                shape = RoundedCornerShape(12.dp)
                            ) {
                                if (state is ConnectionState.Connecting) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.size(24.dp),
                                        color = Color.White,
                                        strokeWidth = 2.dp
                                    )
                                    Spacer(modifier = Modifier.width(12.dp))
                                    Text("Connecting...")
                                } else {
                                    Text("Connect to Desktop", fontSize = 16.sp, fontWeight = FontWeight.Bold)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
