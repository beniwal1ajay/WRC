package com.example.ward.data

import android.content.Context
import android.content.SharedPreferences
import com.example.ward.protocol.WardMessage

/**
 * Representation of a paired host desktop PC.
 * Decouples device identity from raw IP network addresses.
 */
data class PairedDevice(
    val deviceId: String = "",
    val deviceName: String = "AJAY-UBUNTU",
    val publicKeyHex: String = "",
    val lastKnownHost: String = "10.31.1.234",
    val lastKnownPort: Int = 9740,
    val isPaired: Boolean = true
)

/**
 * Persistent store for paired host devices.
 */
class PairedDeviceStore(context: Context) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    companion object {
        private const val PREFS_NAME = "ward_device_prefs"
        private const val KEY_DEVICE_ID = "device_id"
        private const val KEY_DEVICE_NAME = "device_name"
        private const val KEY_PUBLIC_KEY = "public_key_hex"
        private const val KEY_LAST_HOST = "last_known_host"
        private const val KEY_LAST_PORT = "last_known_port"
        private const val KEY_IS_PAIRED = "is_paired"

        const val DEFAULT_DEVICE_NAME = "AJAY-UBUNTU"
        const val DEFAULT_HOST = "10.31.1.234"
        const val DEFAULT_PORT = 9740

        fun byteArrayToHex(bytes: ByteArray): String {
            val sb = StringBuilder()
            for (b in bytes) {
                sb.append(String.format("%02x", b))
            }
            return sb.toString()
        }
    }

    fun getPairedDevice(): PairedDevice {
        val deviceId = prefs.getString(KEY_DEVICE_ID, "") ?: ""
        val deviceName = prefs.getString(KEY_DEVICE_NAME, DEFAULT_DEVICE_NAME) ?: DEFAULT_DEVICE_NAME
        val publicKeyHex = prefs.getString(KEY_PUBLIC_KEY, "") ?: ""
        val lastHost = prefs.getString(KEY_LAST_HOST, DEFAULT_HOST) ?: DEFAULT_HOST
        val lastPort = prefs.getInt(KEY_LAST_PORT, DEFAULT_PORT)
        val isPaired = prefs.getBoolean(KEY_IS_PAIRED, true)

        return PairedDevice(
            deviceId = deviceId,
            deviceName = deviceName,
            publicKeyHex = publicKeyHex,
            lastKnownHost = lastHost,
            lastKnownPort = lastPort,
            isPaired = isPaired
        )
    }

    fun savePairedDevice(device: PairedDevice) {
        prefs.edit()
            .putString(KEY_DEVICE_ID, device.deviceId)
            .putString(KEY_DEVICE_NAME, device.deviceName)
            .putString(KEY_PUBLIC_KEY, device.publicKeyHex)
            .putString(KEY_LAST_HOST, device.lastKnownHost)
            .putInt(KEY_LAST_PORT, device.lastKnownPort)
            .putBoolean(KEY_IS_PAIRED, device.isPaired)
            .apply()
    }

    fun updateFromSession(
        accepted: WardMessage.SessionAccepted,
        host: String,
        port: Int
    ): PairedDevice {
        val deviceIdHex = byteArrayToHex(accepted.serverDeviceId)
        val deviceName = if (accepted.serverDeviceName.isNotBlank()) {
            accepted.serverDeviceName
        } else {
            DEFAULT_DEVICE_NAME
        }

        val updated = PairedDevice(
            deviceId = deviceIdHex,
            deviceName = deviceName,
            publicKeyHex = deviceIdHex,
            lastKnownHost = host,
            lastKnownPort = port,
            isPaired = true
        )
        savePairedDevice(updated)
        return updated
    }
}
