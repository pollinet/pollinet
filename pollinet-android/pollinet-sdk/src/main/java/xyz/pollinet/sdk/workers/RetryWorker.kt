package xyz.pollinet.sdk.workers

import android.content.Context
import androidx.work.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import xyz.pollinet.sdk.PolliNetSDK
import java.util.concurrent.TimeUnit

/**
 * WorkManager worker for processing retry queue
 * Battery-optimized: Runs every 15 minutes with network constraints
 * 
 * Phase 4.6: Retry Queue - WorkManager Implementation
 */
class RetryWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {
    
    companion object {
        private const val WORK_NAME = "pollinet_retry_worker"
        private const val TAG = "PolliNet.RetryWorker"
        
        /**
         * Schedule periodic retry work
         * Called once during SDK initialization
         */
        fun schedule(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED) // Only when online
                .setRequiresBatteryNotLow(true)               // Respect battery state
                .build()
            
            val retryWork = PeriodicWorkRequestBuilder<RetryWorker>(
                15, TimeUnit.MINUTES // Android minimum for periodic work
            )
                .setConstraints(constraints)
                .setBackoffCriteria(
                    BackoffPolicy.EXPONENTIAL,
                    WorkRequest.MIN_BACKOFF_MILLIS,
                    TimeUnit.MILLISECONDS
                )
                .addTag(TAG)
                .build()
            
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork(
                    WORK_NAME,
                    ExistingPeriodicWorkPolicy.KEEP,
                    retryWork
                )
            
            android.util.Log.i(TAG, "Retry worker scheduled (every 15 minutes)")
        }
        
        /**
         * Cancel retry work
         */
        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME)
            android.util.Log.i(TAG, "Retry worker cancelled")
        }
    }
    
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        android.util.Log.d(TAG, "Retry worker starting...")
        
        try {
            // TODO: Get SDK instance from service
            // For now, this is a placeholder
            // In real implementation, we'd get the SDK from a singleton or service
            
            var processedCount = 0
            var successCount = 0
            
            android.util.Log.i(TAG, "Processed: $processedCount, Succeeded: $successCount")
            
            // Return success with metrics
            val outputData = workDataOf(
                "processed" to processedCount,
                "succeeded" to successCount,
                "timestamp" to System.currentTimeMillis()
            )
            
            Result.success(outputData)
            
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Retry worker failed", e)
            Result.retry() // Will retry with exponential backoff
        }
    }
}

