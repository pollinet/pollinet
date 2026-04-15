import SwiftUI

struct ContentView: View {
    @EnvironmentObject var sdkManager: SdkManager
    @State private var selectedTab = 0

    var body: some View {
        TabView(selection: $selectedTab) {
            DashboardView()
                .tabItem { Label("Dashboard", systemImage: "antenna.radiowaves.left.and.right") }
                .tag(0)

            LogsView()
                .tabItem { Label("Logs", systemImage: "list.bullet.rectangle") }
                .tag(1)

            SettingsView()
                .tabItem { Label("Settings", systemImage: "gear") }
                .tag(2)
        }
        .environmentObject(sdkManager)
    }
}
