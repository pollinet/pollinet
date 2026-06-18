package xyz.pollinet.sdk.workers

import android.content.Context
import androidx.work.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import xyz.pollinet.sdk.SdkHolder
import java.util.concurrent.TimeUnit

/**
 * WorkManager worker for processing retry queue.
 * Battery-optimized: runs every 15 minutes with network constraints.
 *
 * When the BleService is alive SdkHolder.get() returns the running SDK instance.
 * The worker calls sdk.tick() to advance the Rust state machine, then drains
 * popReadyRetry() so any items whose back-off has expired are surfaced.
 * BleService will pick them up via nextOutbound() on the next send cycle.
 * If the SDK is gone (service destroyed) the worker exits cleanly and tries again
 * on the next 15-minute interval.
 */
class RetryWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        private const val WORK_NAME = "pollinet_retry_worker"
        private const val TAG = "PolliNet.RetryWorker"

        fun schedule(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                // Battery constraint intentionally omitted: retrying failed transactions
                // is time-sensitive and must not be skipped just because battery is low.
                .build()

            val retryWork = PeriodicWorkRequestBuilder<RetryWorker>(
                15, TimeUnit.MINUTES
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

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME)
            android.util.Log.i(TAG, "Retry worker cancelled")
        }
    }

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        android.util.Log.d(TAG, "Retry worker starting...")

        val sdk = SdkHolder.get() ?: run {
            android.util.Log.i(TAG, "SDK not available — service not running, skipping retry pass")
            return@withContext Result.success(workDataOf("skipped" to true))
        }

        return@withContext try {
            // Advance the Rust state machine so back-off timers are evaluated
            sdk.tick().getOrElse { emptyList() }

            // Drain all retry items whose back-off has expired
            var processed = 0
            var failed = 0
            while (true) {
                val item = sdk.popReadyRetry().getOrNull() ?: break
                processed++
                android.util.Log.d(TAG, "Retry item ready: txId=${item.txId} attempt=${item.attemptCount} lastError=${item.lastError}")
                // The Rust layer re-queues the item into the outbound pipeline after popping;
                // BleService will pick it up via nextOutbound() on the next send cycle.
            }

            android.util.Log.i(TAG, "Retry worker done — processed=$processed failed=$failed")
            Result.success(
                workDataOf(
                    "processed" to processed,
                    "failed" to failed,
                    "timestamp" to System.currentTimeMillis()
                )
            )
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Retry worker failed", e)
            Result.retry()
        }
    }
}
