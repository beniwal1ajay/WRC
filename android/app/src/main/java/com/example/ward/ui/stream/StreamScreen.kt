package com.example.ward.ui.stream

import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import com.example.ward.decoder.VideoDecoder
import com.example.ward.network.ConnectionState
import com.example.ward.network.WardClient
import com.example.ward.protocol.WardMessage

@Composable
fun WardRemoteApp() {
    var host by remember { mutableStateOf("10.0.2.2") } // 10.0.2.2 points to host machine from emulator
    var portText by remember { mutableStateOf("9740") }
    var connectionState by remember { mutableStateOf<ConnectionState>(ConnectionState.Disconnected) }
    var fps by remember { mutableIntStateOf(0) }
    var streamWidth by remember { mutableIntStateOf(0) }
    var streamHeight by remember { mutableIntStateOf(0) }
    var showControls by remember { mutableStateOf(true) }

    var videoDecoder: VideoDecoder? by remember { mutableStateOf(null) }
    var wardClient: WardClient? by remember { mutableStateOf(null) }

    DisposableEffect(Unit) {
        onDispose {
            wardClient?.disconnect()
            videoDecoder?.stop()
        }
    }

    Box(modifier = Modifier.fillMaxSize().background(Color(0xFF121212))) {
        when (val state = connectionState) {
            is ConnectionState.Connected -> {
                // Video SurfaceView with touch controls
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
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
                                            wardClient?.sendInput(WardMessage.MouseButton(1, 1)) // Press
                                            wardClient?.sendInput(WardMessage.MouseButton(1, 0)) // Release
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

                // Top Floating Overlay Bar (Stats & Quick Actions)
                AnimatedVisibility(
                    visible = showControls,
                    enter = fadeIn(),
                    exit = fadeOut(),
                    modifier = Modifier.align(Alignment.TopCenter).padding(top = 36.dp, start = 16.dp, end = 16.dp)
                ) {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = Color(0xCC1E1E1E)),
                        shape = RoundedCornerShape(16.dp),
                        elevation = CardDefaults.cardElevation(8.dp)
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            Text(
                                text = "${streamWidth}x${streamHeight}",
                                color = Color(0xFF90CAF9),
                                fontSize = 13.sp,
                                fontWeight = FontWeight.Bold
                            )
                            Text(
                                text = "$fps FPS",
                                color = if (fps >= 30) Color(0xFF81C784) else Color(0xFFFFB74D),
                                fontSize = 13.sp,
                                fontWeight = FontWeight.Bold
                            )

                            Button(
                                onClick = { wardClient?.requestKeyframe() },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF37474F)),
                                contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                modifier = Modifier.height(32.dp)
                            ) {
                                Text("Refresh", fontSize = 12.sp)
                            }

                            Button(
                                onClick = {
                                    wardClient?.disconnect()
                                    videoDecoder?.stop()
                                    connectionState = ConnectionState.Disconnected
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFD32F2F)),
                                contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                modifier = Modifier.height(32.dp)
                            ) {
                                Text("Disconnect", fontSize = 12.sp)
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
