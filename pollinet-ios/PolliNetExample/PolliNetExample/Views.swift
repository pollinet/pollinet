//
//  Views.swift
//  The four QA screens, mirroring pollinet-android + Pollistem's Diagnostics:
//  Setup (init/config), Send (approve + offline intent), Relay (console),
//  Mesh (peers / metrics / transport controls).
//

import PolliNetSDK
import SwiftUI

// MARK: - Setup

struct SetupView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            Form {
                Section("Status") {
                    Text(model.status).font(.footnote.monospaced())
                    if let error = model.lastError {
                        Text(error).font(.footnote).foregroundStyle(.red)
                    }
                }
                Section("Config") {
                    TextField("Solana RPC URL", text: $model.rpcUrl)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    TextField("Token mint (base58)", text: $model.tokenMint)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                }
                Section("Dev wallet (Ed25519, QA only)") {
                    Text(model.walletAddress)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                }
                Section {
                    if model.isInitialized {
                        Button("Shutdown SDK", role: .destructive) { model.shutdown() }
                    } else {
                        Button("Initialize SDK") { Task { await model.initializeSDK() } }
                    }
                }
            }
            .navigationTitle("PolliNet Setup")
        }
    }
}

// MARK: - Send (approve + offline intent)

struct SendView: View {
    @EnvironmentObject private var model: AppModel
    @State private var recipient = ""
    @State private var amount = ""
    @State private var result = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Delegated token accounts") {
                    Button("Refresh") { Task { await model.refreshTokenAccounts() } }
                        .disabled(!model.isInitialized)
                    ForEach(model.tokenAccounts, id: \.pubkey) { account in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(account.mint.prefix(12) + "… · balance \(account.rawBalance)")
                                .font(.footnote.monospaced())
                            Text(account.isExecutorDelegated ? "✓ Pollinet-ready" : "not delegated")
                                .font(.caption2)
                                .foregroundStyle(account.isExecutorDelegated ? .green : .orange)
                        }
                        .swipeActions {
                            Button("Approve") {
                                Task {
                                    do {
                                        let signature = try await model.approve(
                                            tokenAccount: account.pubkey,
                                            mint: account.mint,
                                            amount: account.rawBalance,
                                            decimals: account.decimals
                                        )
                                        result = "Approved: \(signature.prefix(20))…"
                                    } catch { result = "Approve failed: \(error.localizedDescription)" }
                                }
                            }
                        }
                    }
                }
                Section("Offline intent") {
                    TextField("Recipient wallet (base58)", text: $recipient)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    TextField("Amount (smallest unit)", text: $amount)
                        .keyboardType(.numberPad)
                    Button("Sign & queue for mesh") {
                        Task {
                            do {
                                let txId = try await model.sendIntent(
                                    toWallet: recipient,
                                    mint: model.tokenMint,
                                    amount: Int64(amount) ?? 0,
                                    expiresInSeconds: 3600
                                )
                                result = "Queued \(txId.prefix(16))… for propagation"
                            } catch { result = "Send failed: \(error.localizedDescription)" }
                        }
                    }
                    .disabled(!model.isInitialized || recipient.isEmpty || amount.isEmpty || model.tokenMint.isEmpty)
                }
                if !result.isEmpty {
                    Section("Result") {
                        Text(result).font(.footnote.monospaced()).textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("Send")
        }
    }
}

// MARK: - Relay console

struct RelayView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                if let ble = model.ble {
                    RelayContent(ble: ble)
                } else {
                    Text("Initialize the SDK first").foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Relay")
        }
    }
}

private struct RelayContent: View {
    @ObservedObject var ble: BleController

    var body: some View {
        Section("Received transactions") {
            if ble.receivedTransactions.isEmpty {
                Text("None yet").foregroundStyle(.secondary)
            }
            ForEach(ble.receivedTransactions) { record in
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(record.id.prefix(16))…").font(.footnote.monospaced())
                    Text("\(record.status.rawValue) · \(record.detail)")
                        .font(.caption2)
                        .foregroundStyle(record.status == .failed ? .red : .secondary)
                }
            }
        }
        Section("Confirmations") {
            if ble.confirmationLog.isEmpty {
                Text("None yet").foregroundStyle(.secondary)
            }
            ForEach(ble.confirmationLog) { record in
                Label(
                    "\(record.txIdShort)… \(record.detail)",
                    systemImage: record.success ? "checkmark.circle" : "xmark.circle"
                )
                .font(.footnote.monospaced())
                .foregroundStyle(record.success ? .green : .red)
            }
        }
        Section("Log") {
            ForEach(Array(ble.logs.suffix(60).enumerated()), id: \.offset) { _, line in
                Text(line).font(.caption2.monospaced())
            }
        }
    }
}

// MARK: - Mesh (peers / metrics / transport controls)

struct MeshView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                if let ble = model.ble {
                    MeshContent(ble: ble, multipeer: model.multipeer)
                } else {
                    Text("Initialize the SDK first").foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Mesh")
        }
    }
}

private struct MeshContent: View {
    @ObservedObject var ble: BleController
    var multipeer: MultipeerController?

    var body: some View {
        Section("BLE — \(ble.connectionState.rawValue)") {
            Toggle("Scan", isOn: Binding(
                get: { ble.isScanning },
                set: { $0 ? ble.startScanning() : ble.stopScanning() }
            ))
            Toggle("Advertise", isOn: Binding(
                get: { ble.isAdvertising },
                set: { $0 ? ble.startAdvertising() : ble.stopAdvertising() }
            ))
        }
        if let multipeer {
            MultipeerSection(multipeer: multipeer)
        }
        Section("Peers") {
            if ble.peers.isEmpty { Text("None discovered").foregroundStyle(.secondary) }
            ForEach(ble.peers.values.sorted { $0.lastSeenAt > $1.lastSeenAt }) { peer in
                HStack {
                    Circle()
                        .fill(peer.isConnected ? Color.green : Color.gray)
                        .frame(width: 8, height: 8)
                    Text("\(peer.id.prefix(8))…").font(.footnote.monospaced())
                    Spacer()
                    Text("\(peer.rssi) dBm").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        Section("Metrics") {
            if let metrics = ble.latestMetrics {
                Text("buffered \(metrics.fragmentsBuffered) · complete \(metrics.transactionsComplete) · failures \(metrics.reassemblyFailures)")
                    .font(.footnote.monospaced())
            } else {
                Text("No metrics yet").foregroundStyle(.secondary)
            }
        }
    }
}

private struct MultipeerSection: View {
    @ObservedObject var multipeer: MultipeerController
    @State private var running = false

    var body: some View {
        Section("Multipeer — \(multipeer.linkStatus.rawValue) (\(multipeer.connectedPeers) peer(s))") {
            Toggle("iOS↔iOS transport", isOn: $running)
                .onChange(of: running) { on in
                    on ? multipeer.start() : multipeer.stop()
                }
        }
    }
}
