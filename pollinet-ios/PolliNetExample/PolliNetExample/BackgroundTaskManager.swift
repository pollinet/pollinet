import BackgroundTasks
import UIKit

// MARK: - Background task identifiers
// These must match the entries in Info.plist > BGTaskSchedulerPermittedIdentifiers

private enum TaskIdentifier {
    static let appRefresh = "xyz.pollinet.example.refresh"
    static let processing = "xyz.pollinet.example.processing"
}

// MARK: - BackgroundTaskManager

enum BackgroundTaskManager {

    // MARK: Registration (call from App.init before first scene is connected)

    static func register() {
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: TaskIdentifier.appRefresh,
            using: nil
        ) { task in
            handleAppRefresh(task: task as! BGAppRefreshTask)
        }

        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: TaskIdentifier.processing,
            using: nil
        ) { task in
            handleProcessing(task: task as! BGProcessingTask)
        }
    }

    // MARK: Schedule

    static func scheduleAll() {
        scheduleAppRefresh()
        scheduleProcessing()
    }

    private static func scheduleAppRefresh() {
        let request = BGAppRefreshTaskRequest(identifier: TaskIdentifier.appRefresh)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60) // 15 min
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func scheduleProcessing() {
        let request = BGProcessingTaskRequest(identifier: TaskIdentifier.processing)
        request.requiresNetworkConnectivity = true
        request.requiresExternalPower = false
        request.earliestBeginDate = Date(timeIntervalSinceNow: 60 * 60) // 1 hour
        try? BGTaskScheduler.shared.submit(request)
    }

    // MARK: Handlers

    private static func handleAppRefresh(task: BGAppRefreshTask) {
        scheduleAppRefresh() // reschedule before doing work

        let workTask = Task {
            // Obtain manager from the active scene
            guard let sdkManager = activeSdkManager() else {
                task.setTaskCompleted(success: true)
                return
            }
            await sdkManager.processRetryQueue()
            await sdkManager.runCleanup()
            task.setTaskCompleted(success: true)
        }

        task.expirationHandler = {
            workTask.cancel()
            task.setTaskCompleted(success: false)
        }
    }

    private static func handleProcessing(task: BGProcessingTask) {
        scheduleProcessing()

        let workTask = Task {
            guard let sdkManager = activeSdkManager() else {
                task.setTaskCompleted(success: true)
                return
            }
            await sdkManager.processRetryQueue()
            await sdkManager.drainConfirmationQueue()
            await sdkManager.runCleanup()
            task.setTaskCompleted(success: true)
        }

        task.expirationHandler = {
            workTask.cancel()
            task.setTaskCompleted(success: false)
        }
    }

    // MARK: Helper — locate SdkManager from active scene

    @MainActor
    private static func activeSdkManager() -> SdkManager? {
        // Walk the connected scenes to find the SwiftUI environment object
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene,
                  let rootVC = windowScene.windows.first?.rootViewController else { continue }
            // SwiftUI injects environment objects via UIHostingController
            if let hosting = rootVC as? UIViewController,
               let sdkManager = Mirror(reflecting: hosting)
                   .children
                   .compactMap({ $0.value as? SdkManager })
                   .first {
                return sdkManager
            }
        }
        return nil
    }
}
