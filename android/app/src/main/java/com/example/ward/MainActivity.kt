package com.example.ward

import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.example.ward.theme.WARDTheme
import com.example.ward.ui.stream.WardRemoteApp

class MainActivity : ComponentActivity() {

    companion object {
        var currentInstance: MainActivity? = null
            private set
    }

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { /* Permission result handled */ }

    private var isImmersiveRequested = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        currentInstance = this

        try {
            WindowCompat.setDecorFitsSystemWindows(window, false)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                val lp = window.attributes
                lp.layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                window.attributes = lp
            }
        } catch (e: Throwable) {
            android.util.Log.w("MainActivity", "Cutout setup error: ${e.message}")
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            try {
                notificationPermissionLauncher.launch(android.Manifest.permission.POST_NOTIFICATIONS)
            } catch (e: Throwable) {
                android.util.Log.w("MainActivity", "Permission error: ${e.message}")
            }
        }

        try {
            @Suppress("DEPRECATION")
            window.decorView.setOnSystemUiVisibilityChangeListener { visibility ->
                if (isImmersiveRequested && (visibility and View.SYSTEM_UI_FLAG_FULLSCREEN) == 0) {
                    window.decorView.postDelayed({
                        if (isImmersiveRequested) {
                            applyImmersiveMode(true)
                        }
                    }, 300)
                }
            }
        } catch (e: Throwable) {
            android.util.Log.w("MainActivity", "VisibilityListener error: ${e.message}")
        }

        setContent {
            WARDTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    WardRemoteApp()
                }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        if (currentInstance == this) {
            currentInstance = null
        }
    }

    /**
     * Controls fullscreen immersive sticky mode.
     * When enabled, hides Android status and navigation bars, allowing full display utilization
     * and preventing system UI from occupying or overlaying the remote desktop.
     */
    fun setImmersiveMode(enable: Boolean) {
        isImmersiveRequested = enable
        runOnUiThread {
            applyImmersiveMode(enable)
            window.decorView.post {
                if (isImmersiveRequested == enable) {
                    applyImmersiveMode(enable)
                }
            }
        }
    }

    private fun applyImmersiveMode(enable: Boolean) {
        try {
            val windowInsetsController = WindowCompat.getInsetsController(window, window.decorView)
            if (enable) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    val lp = window.attributes
                    lp.layoutInDisplayCutoutMode =
                        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                    window.attributes = lp
                }
                window.clearFlags(WindowManager.LayoutParams.FLAG_FORCE_NOT_FULLSCREEN)
                window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)

                windowInsetsController.systemBarsBehavior =
                    WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                windowInsetsController.hide(WindowInsetsCompat.Type.systemBars())
                windowInsetsController.hide(WindowInsetsCompat.Type.statusBars())
                windowInsetsController.hide(WindowInsetsCompat.Type.navigationBars())

                @Suppress("DEPRECATION")
                window.decorView.systemUiVisibility = (
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    or View.SYSTEM_UI_FLAG_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                )
            } else {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    val lp = window.attributes
                    lp.layoutInDisplayCutoutMode =
                        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_DEFAULT
                    window.attributes = lp
                }
                window.clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)

                windowInsetsController.show(WindowInsetsCompat.Type.systemBars())
                windowInsetsController.show(WindowInsetsCompat.Type.statusBars())
                windowInsetsController.show(WindowInsetsCompat.Type.navigationBars())

                @Suppress("DEPRECATION")
                window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_VISIBLE
            }
        } catch (e: Throwable) {
            android.util.Log.w("MainActivity", "applyImmersiveMode error: ${e.message}")
        }
    }

    override fun onConfigurationChanged(newConfig: android.content.res.Configuration) {
        super.onConfigurationChanged(newConfig)
        if (isImmersiveRequested) {
            window.decorView.post { applyImmersiveMode(true) }
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && isImmersiveRequested) {
            applyImmersiveMode(true)
        }
    }

    override fun onResume() {
        super.onResume()
        if (isImmersiveRequested) {
            applyImmersiveMode(true)
        }
    }

    /**
     * Controls screen keep-awake flag.
     * When true, keeps display on during active remote desktop streaming.
     */
    fun setKeepScreenOn(enable: Boolean) {
        runOnUiThread {
            if (enable) {
                window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            } else {
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            }
        }
    }
}
