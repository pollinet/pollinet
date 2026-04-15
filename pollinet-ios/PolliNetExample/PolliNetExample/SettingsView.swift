import SwiftUI

struct SettingsView: View {
    @EnvironmentObject var sdkManager: SdkManager
    @State private var draftWallet = ""
    @State private var showRestartAlert = false

    var body: some View {
        NavigationView {
            Form {
                Section("Wallet") {
                    TextField("Base58 wallet address", text: $draftWallet)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .font(.system(.body, design: .monospaced))
                        .onAppear { draftWallet = sdkManager.walletAddress }

                    Button("Apply") {
                        sdkManager.walletAddress = draftWallet
                        Task { await sdkManager.applyWalletAddress() }
                    }
                    .disabled(draftWallet == sdkManager.walletAddress)
                }

                Section("SDK") {
                    LabeledContent("Version", value: sdkManager.sdkVersion)
                    LabeledContent("Status", value: sdkManager.isInitialized ? "Online" : "Offline")

                    Button("Restart SDK", role: .destructive) {
                        showRestartAlert = true
                    }
                }

                Section("Background Tasks") {
                    Button("Schedule Background Refresh") {
                        BackgroundTaskManager.scheduleAll()
                    }
                }

                Section("Maintenance") {
                    Button("Run Cleanup") {
                        Task { await sdkManager.runCleanup() }
                    }
                }
            }
            .navigationTitle("Settings")
            .alert("Restart SDK?", isPresented: $showRestartAlert) {
                Button("Restart", role: .destructive) {
                    sdkManager.shutdown()
                    Task { await sdkManager.initializeSdk() }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("All in-memory state will be cleared.")
            }
        }
    }
}
