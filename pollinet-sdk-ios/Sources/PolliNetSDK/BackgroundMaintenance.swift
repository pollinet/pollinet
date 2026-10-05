//
//  BackgroundMaintenance.swift
//  BGTaskScheduler replacements for Android's WorkManager jobs:
//    RetryWorker   (15 min): tick() + drain popReadyRetry
//    CleanupWorker (30 min): cleanupStaleFragments + cleanupExpired + cleanupOldSubmissions
//
//  iOS BGTasks are BEST-EFFORT — the OS decides when (if ever) they run. The
//  Android cadences are requested as earliestBeginDate, not guaranteed. To
//  compensate, call `runRetryPass()` + `runCleanupPass()` on every foreground
//  activation and on CoreBluetooth state-restoration wake (see plan-ios.md
//  Phase 5.1) — the host app owns those hooks.
//
//  App integration checklist:
//    1. Info.plist → BGTaskSchedulerPermittedIdentifiers:
//         xyz.pollinet.retry, xyz.pollinet.cleanup
//    2. Call BackgroundMaintenance.register(sdkProvider:) BEFORE the app
//       finishes launching (BGTaskScheduler requirement).
//    3. Call BackgroundMaintenance.scheduleAll() on scenePhase → .background.
//

import Foundation

#if os(iOS)
import BackgroundTasks
#endif

public enum BackgroundMaintenance {

    public static let retryTaskIdentifier = "xyz.pollinet.retry"
    public static let cleanupTaskIdentifier = "xyz.pollinet.cleanup"

    /// Android cadences, requested (not guaranteed) via earliestBeginDate.
    public static let retryInterval: TimeInterval = 15 * 60
    public static let cleanupInterval: TimeInterval = 30 * 60

    #if os(iOS)
    /// Register both task handlers. Must run before the app finishes launching.
    /// `sdkProvider` returns the live SDK (or nil when not yet initialized).
    public static func register(sdkProvider: @escaping @Sendable () -> PolliNetSDK?) {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: retryTaskIdentifier, using: nil) { task in
            handle(task: task, reschedule: scheduleRetry) { sdk in
                await runRetryPass(sdk: sdk)
            } sdkProvider: { sdkProvider() }
        }
        BGTaskScheduler.shared.register(forTaskWithIdentifier: cleanupTaskIdentifier, using: nil) { task in
            handle(task: task, reschedule: scheduleCleanup) { sdk in
                await runCleanupPass(sdk: sdk)
            } sdkProvider: { sdkProvider() }
        }
    }

    /// Schedule (or re-schedule) both maintenance tasks.
    public static func scheduleAll() {
        scheduleRetry()
        scheduleCleanup()
    }

    private static func scheduleRetry() {
        let request = BGProcessingTaskRequest(identifier: retryTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: retryInterval)
        request.requiresNetworkConnectivity = true // retries need the RPC path
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func scheduleCleanup() {
        let request = BGProcessingTaskRequest(identifier: cleanupTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: cleanupInterval)
        request.requiresNetworkConnectivity = false
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func handle(
        task: BGTask,
        reschedule: @escaping @Sendable () -> Void,
        pass: @escaping @Sendable (PolliNetSDK) async -> Void,
        sdkProvider: @escaping @Sendable () -> PolliNetSDK?
    ) {
        reschedule() // keep the chain alive regardless of outcome
        guard let sdk = sdkProvider() else {
            task.setTaskCompleted(success: false)
            return
        }
        let work = Task {
            await pass(sdk)
            task.setTaskCompleted(success: true)
        }
        task.expirationHandler = { work.cancel() }
    }
    #endif

    // MARK: - Maintenance passes (platform-independent; also called on
    // foreground activation / BLE restoration wake)

    /// RetryWorker counterpart: engine tick + drain ready retries back into the
    /// confirmation pipeline (submission happens over the live transport loops;
    /// here we only re-queue, matching RetryWorker's tick-and-drain shape).
    public static func runRetryPass(sdk: PolliNetSDK) async {
        _ = try? await sdk.tick()
        while let retry = try? await sdk.popReadyRetry() {
            // Without a live link, push the item straight back with its error so
            // backoff keeps growing; the BLE/Multipeer worker submits when online.
            try? await sdk.addToRetryQueue(
                txBytes: Data(base64Encoded: retry.txBytes) ?? Data(),
                txId: retry.txId,
                error: retry.lastError
            )
            break // one rotation per pass — avoid a tight self-feeding loop
        }
        try? await sdk.autoSaveQueues()
    }

    /// CleanupWorker counterpart.
    public static func runCleanupPass(sdk: PolliNetSDK) async {
        _ = try? await sdk.cleanupStaleFragments()
        _ = try? await sdk.cleanupExpired()
        _ = try? await sdk.cleanupOldSubmissions()
        try? await sdk.periodicMaintenance()
        try? await sdk.autoSaveQueues()
    }
}
