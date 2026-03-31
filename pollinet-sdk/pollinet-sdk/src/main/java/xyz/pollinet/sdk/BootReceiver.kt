package xyz.pollinet.sdk

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log

/**
 * Broadcast receiver that automatically starts the PolliNet BLE service
 * after device boot completes.
 * 
 * This ensures the SDK continues running even after device reboots,
 * maintaining the mesh network functionality.
 */
class BootReceiver : BroadcastReceiver() {
    
    companion object {
        private const val TAG = "PolliNet.BootReceiver"
    }
    
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED -> {
                Log.d(TAG, "Device boot completed - starting PolliNet BLE service")
                startBleService(context, "after boot")
            }
            Intent.ACTION_MY_PACKAGE_REPLACED -> {
                Log.d(TAG, "App updated - restarting PolliNet BLE service")
                startBleService(context, "after app update")
            }
        }
    }

    private fun startBleService(context: Context, reason: String) {
        try {
            val serviceIntent = Intent(context, BleService::class.java).apply {
                action = BleService.ACTION_START
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent)
            } else {
                context.startService(serviceIntent)
            }
            Log.d(TAG, "✅ Service started $reason")
        } catch (e: Exception) {
            Log.e(TAG, "❌ Failed to start service $reason", e)
        }
    }
}

