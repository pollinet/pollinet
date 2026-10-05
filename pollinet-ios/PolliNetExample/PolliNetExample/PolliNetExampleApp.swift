//
//  PolliNetExampleApp.swift
//  Reference + QA app for the PolliNet iOS SDK (counterpart of pollinet-android).
//

import PolliNetSDK
import SwiftUI

@main
struct PolliNetExampleApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var scenePhase

    init() {
        // BGTask handlers must be registered before launch finishes.
        BackgroundMaintenance.register { AppModel.sharedSDK }
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
        }
        .onChange(of: scenePhase) { phase in
            switch phase {
            case .background:
                BackgroundMaintenance.scheduleAll()
            case .active:
                // BGTasks are best-effort — run both passes on every activation.
                if let sdk = AppModel.sharedSDK {
                    Task {
                        await BackgroundMaintenance.runRetryPass(sdk: sdk)
                        await BackgroundMaintenance.runCleanupPass(sdk: sdk)
                    }
                }
            default:
                break
            }
        }
    }
}

struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        TabView {
            SetupView()
                .tabItem { Label("Setup", systemImage: "gearshape") }
            SendView()
                .tabItem { Label("Send", systemImage: "paperplane") }
            RelayView()
                .tabItem { Label("Relay", systemImage: "arrow.triangle.2.circlepath") }
            MeshView()
                .tabItem { Label("Mesh", systemImage: "dot.radiowaves.left.and.right") }
        }
    }
}
