package xyz.pollinet.sdk

import java.lang.ref.WeakReference

/**
 * Process-scoped holder for the running PolliNetSDK instance.
 *
 * BleService sets this when the SDK is initialised so that WorkManager
 * background tasks (RetryWorker, CleanupWorker) can reach the same
 * instance without needing a direct service binding.
 *
 * A WeakReference is used deliberately: if BleService is destroyed the
 * SDK can be GC'd without a leak.  Workers check for null and return
 * Result.success() with zero work done — they will run again on the
 * next scheduled interval.
 */
internal object SdkHolder {
    private var ref: WeakReference<PolliNetSDK>? = null

    fun set(sdk: PolliNetSDK) {
        ref = WeakReference(sdk)
    }

    fun get(): PolliNetSDK? = ref?.get()

    fun clear() {
        ref = null
    }
}
