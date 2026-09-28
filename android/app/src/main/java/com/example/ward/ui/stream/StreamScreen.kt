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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
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
private const val KEYSYM_LEFT = 0xFF51
private const val KEYSYM_UP = 0xFF52
private const val KEYSYM_RIGHT = 0xFF53
private const val KEYSYM_DOWN = 0xFF54
private const val KEYSYM_SUPER = 0xFFEB

// Linux evdev keycodes
private const val EVDEV_KEY_LEFTCTRL = 29
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
    var showKeyboardBar by remember { mutableStateOf(false) }
    var textInput by remember { mutableStateOf("") }

    var videoDecoder: VideoDecoder? by remember { mutableStateOf(null) }
    var wardClient: WardClient? by remember { mutableStateOf(null) }

    fun sendKeysym(keysym: Int) {
        val code = keysym or KEYSYM_FLAG
        wardClient?.sendInput(WardMessage.KeyDown(code))
        wardClient?.sendInput(WardMessage.KeyUp(code))
    }

    fun sendText(text: String) {
        for (ch in text) {
            val keysym = ch.code
            sendKeysym(keysym)
        }
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
                // Outer container with constraints to maintain exact aspect ratio
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

                    // Constrain without stretching in either portrait or landscape
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
                                        // Request immediate IDR keyframe so video renders instantly
                                        wardClient?.requestKeyframe()
                                    }

                                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

                                    override fun surfaceDestroyed(holder: SurfaceHolder) {
                                        videoDecoder?.stop()
                                        videoDecoder = null
                                    }
                                })

                                // Touch event dispatcher for remote desktop control
                                var lastX = 0f
                                var lastY = 0f
                                var isDown = false
                                var downTime = 0L

                                setOnTouchListener { v, event ->
                                    when (event.actionMasked) {
                                        MotionEvent.ACTION_DOWN -> {
                                            lastX = event.x
                                            lastY = event.y
                                            isDown = true
                                            downTime = System.currentTimeMillis()
                                            v.performClick()
                                            true
                                        }
                                        MotionEvent.ACTION_MOVE -> {
                                            if (event.pointerCount == 1) {
                                                // Relative mouse movement
                                                val dx = event.x - lastX
                                                val dy = event.y - lastY
                                                wardClient?.sendInput(WardMessage.MouseMove(dx, dy))
                                                lastX = event.x
                                                lastY = event.y
                                            } else if (event.pointerCount == 2) {
                                                // Two-finger scroll
                                                val dy = event.y - lastY
                                                val scrollStep = if (dy > 0) 1.0f else -1.0f
                                                wardClient?.sendInput(WardMessage.MouseScroll(0f, scrollStep))
                                                lastY = event.y
                                            }
                                            true
                                        }
                                        MotionEvent.ACTION_UP -> {
                                            val duration = System.currentTimeMillis() - downTime
                                            if (duration < 250) {
                                                // Tap -> Left Click
                                                wardClient?.sendInput(WardMessage.MouseButton(1, 1))
                                                wardClient?.sendInput(WardMessage.MouseButton(1, 0))
                                            }
                                            isDown = false
                                            true
                                        }
                                        else -> false
                                    }
                                }
                            }
                        }
                    )
                }

                // Top Floating Overlay Bar (Stats & Quick Actions)
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

                            Button(
                                onClick = { showKeyboardBar = !showKeyboardBar },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = if (showKeyboardBar) Color(0xFF0288D1) else Color(0xFF37474F)
                                ),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                                modifier = Modifier.height(30.dp)
                            ) {
                                Text("⌨ Keys", fontSize = 11.sp)
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

                // Bottom Keyboard & Shortcut Bar Overlay
                AnimatedVisibility(
                    visible = showKeyboardBar,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 16.dp, start = 8.dp, end = 8.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xEE1E1E1E)),
                        shape = RoundedCornerShape(16.dp),
                        elevation = CardDefaults.cardElevation(10.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Column(
                            modifier = Modifier.padding(10.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            // Text Input Field for typing into remote desktop
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                OutlinedTextField(
                                    value = textInput,
                                    onValueChange = { textInput = it },
                                    placeholder = { Text("Type text here to send...", fontSize = 13.sp) },
                                    singleLine = true,
                                    modifier = Modifier.weight(1f),
                                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                                    keyboardActions = KeyboardActions(onSend = {
                                        if (textInput.isNotEmpty()) {
                                            sendText(textInput)
                                            textInput = ""
                                        }
                                    }),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        focusedTextColor = Color.White,
                                        unfocusedTextColor = Color.White,
                                        focusedBorderColor = Color(0xFF4FC3F7),
                                        unfocusedBorderColor = Color(0xFF546E7A)
                                    )
                                )
                                Button(
                                    onClick = {
                                        if (textInput.isNotEmpty()) {
                                            sendText(textInput)
                                            textInput = ""
                                        }
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0288D1)),
                                    modifier = Modifier.height(48.dp)
                                ) {
                                    Text("Send")
                                }
                            }

                            // Quick keys toolbar
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(6.dp)
                            ) {
                                val quickKeyButton = @Composable { label: String, onClick: () -> Unit ->
                                    Button(
                                        onClick = onClick,
                                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF2C3E50)),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                                        modifier = Modifier.height(32.dp)
                                    ) {
                                        Text(label, fontSize = 11.sp)
                                    }
                                }

                                quickKeyButton("Enter") { sendKeysym(KEYSYM_RETURN) }
                                quickKeyButton("Bksp") { sendKeysym(KEYSYM_BACKSPACE) }
                                quickKeyButton("Space") { sendKeysym(KEYSYM_SPACE) }
                                quickKeyButton("Tab") { sendKeysym(KEYSYM_TAB) }
                                quickKeyButton("Esc") { sendKeysym(KEYSYM_ESCAPE) }
                                quickKeyButton("Super") { sendKeysym(KEYSYM_SUPER) }

                                quickKeyButton("Ctrl+C") {
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_C))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_C))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
                                }
                                quickKeyButton("Ctrl+V") {
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_V))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_V))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
                                }
                                quickKeyButton("Ctrl+Z") {
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_LEFTCTRL))
                                    wardClient?.sendInput(WardMessage.KeyDown(EVDEV_KEY_Z))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_Z))
                                    wardClient?.sendInput(WardMessage.KeyUp(EVDEV_KEY_LEFTCTRL))
                                }

                                quickKeyButton("←") { sendKeysym(KEYSYM_LEFT) }
                                quickKeyButton("↑") { sendKeysym(KEYSYM_UP) }
                                quickKeyButton("→") { sendKeysym(KEYSYM_RIGHT) }
                                quickKeyButton("↓") { sendKeysym(KEYSYM_DOWN) }
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
