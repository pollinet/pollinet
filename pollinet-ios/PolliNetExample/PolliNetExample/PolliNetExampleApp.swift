import SwiftUI

@main
struct PolliNetExampleApp: App {

    @StateObject private var sdkManager = SdkManager()

    init() {
        BackgroundTaskManager.register()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(sdkManager)
        }
    }
}
