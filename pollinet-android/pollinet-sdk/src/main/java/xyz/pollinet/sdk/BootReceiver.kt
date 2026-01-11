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
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.d(TAG, "Device boot completed - starting PolliNet BLE service")
            
            try {
                val serviceIntent = Intent(context, BleService::class.java).apply {
                    action = BleService.ACTION_START
                }
                
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    // Android 8.0+ requires startForegroundService
                    context.startForegroundService(serviceIntent)
                    Log.d(TAG, "✅ Foreground service started after boot")
                } else {
                    // Android 7.1 and below
                    context.startService(serviceIntent)
                    Log.d(TAG, "✅ Service started after boot")
                }
            } catch (e: Exception) {
                Log.e(TAG, "❌ Failed to start service after boot", e)
            }
        } else if (intent.action == Intent.ACTION_MY_PACKAGE_REPLACED) {
            // Also handle app updates - restart service after update
            Log.d(TAG, "App updated - restarting PolliNet BLE service")
            
            try {
                val serviceIntent = Intent(context, BleService::class.java).apply {
                    action = BleService.ACTION_START
                }
                
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    context.startForegroundService(serviceIntent)
                    Log.d(TAG, "✅ Service restarted after app update")
                } else {
                    context.startService(serviceIntent)
                    Log.d(TAG, "✅ Service restarted after app update")
                }
            } catch (e: Exception) {
                Log.e(TAG, "❌ Failed to restart service after update", e)
            }
        }
    }
}

