package xyz.pollinet.sdk.workers

import android.content.Context
import androidx.work.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import xyz.pollinet.sdk.SdkHolder
import java.util.concurrent.TimeUnit

/**
 * WorkManager worker for periodic in-memory / Rust-side cleanup.
 * Battery-optimized: runs every 30 minutes, no network required.
 *
 * Calls the three Rust-side cleanup routines:
 *   • cleanupStaleFragments — removes partially-reassembled fragment sets that
 *     never completed (e.g. sender disappeared mid-transmission).
 *   • cleanupExpired — purges expired confirmation slots and retry entries.
 *   • cleanupOldSubmissions — drops submitted-transaction hashes older than 24 h.
 *
 * If the SDK is gone (service destroyed) the worker exits cleanly.
 */
class CleanupWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        private const val WORK_NAME = "pollinet_cleanup_worker"
        private const val TAG = "PolliNet.CleanupWorker"

        fun schedule(context: Context) {
            val cleanupWork = PeriodicWorkRequestBuilder<CleanupWorker>(
                30, TimeUnit.MINUTES
            )
                .setBackoffCriteria(
                    BackoffPolicy.LINEAR,
                    WorkRequest.MIN_BACKOFF_MILLIS,
                    TimeUnit.MILLISECONDS
                )
                .addTag(TAG)
                .build()

            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork(
                    WORK_NAME,
                    ExistingPeriodicWorkPolicy.KEEP,
                    cleanupWork
                )

            android.util.Log.i(TAG, "Cleanup worker scheduled (every 30 minutes)")
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME)
            android.util.Log.i(TAG, "Cleanup worker cancelled")
        }
    }

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        android.util.Log.d(TAG, "Cleanup worker starting...")

        val sdk = SdkHolder.get() ?: run {
            android.util.Log.i(TAG, "SDK not available — service not running, skipping cleanup")
            return@withContext Result.success(workDataOf("skipped" to true))
        }

        return@withContext try {
            val fragmentsCleaned = sdk.cleanupStaleFragments().getOrElse { 0 }
            val (confirmationsCleaned, retriesCleaned) = sdk.cleanupExpired().getOrElse { Pair(0, 0) }
            sdk.cleanupOldSubmissions()

            android.util.Log.i(
                TAG,
                "Cleanup complete — fragments=$fragmentsCleaned " +
                    "confirmations=$confirmationsCleaned retries=$retriesCleaned"
            )

            Result.success(
                workDataOf(
                    "fragments_cleaned" to fragmentsCleaned,
                    "confirmations_cleaned" to confirmationsCleaned,
                    "retries_cleaned" to retriesCleaned,
                    "timestamp" to System.currentTimeMillis()
                )
            )
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Cleanup worker failed", e)
            Result.retry()
        }
    }
}
