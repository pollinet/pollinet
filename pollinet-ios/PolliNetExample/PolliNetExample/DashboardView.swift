import SwiftUI

struct DashboardView: View {
    @EnvironmentObject var sdkManager: SdkManager

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(spacing: 16) {
                    NodeStatusCard()
                    BleStatusCard()
                    QueueMetricsCard()
                }
                .padding()
            }
            .navigationTitle("PolliNet Node")
            .navigationBarTitleDisplayMode(.large)
        }
    }
}

// MARK: - Node Status Card

private struct NodeStatusCard: View {
    @EnvironmentObject var sdkManager: SdkManager

    var body: some View {
        Card(title: "Node Status") {
            HStack {
                StatusDot(active: sdkManager.isInitialized)
                Text(sdkManager.isInitialized ? "Online" : "Initializing…")
                    .fontWeight(.semibold)
                Spacer()
                Text("v\(sdkManager.sdkVersion)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.top, 4)
        }
    }
}

// MARK: - BLE Status Card

private struct BleStatusCard: View {
    @EnvironmentObject var sdkManager: SdkManager

    var body: some View {
        Card(title: "Bluetooth Mesh") {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Label("Advertising", systemImage: "dot.radiowaves.right")
                        .foregroundStyle(sdkManager.ble.isAdvertising ? .green : .secondary)
                    Spacer()
                    StatusDot(active: sdkManager.ble.isAdvertising)
                }
                HStack {
                    Label("Scanning", systemImage: "dot.radiowaves.left.and.right")
                        .foregroundStyle(sdkManager.ble.isScanning ? .blue : .secondary)
                    Spacer()
                    StatusDot(active: sdkManager.ble.isScanning, color: .blue)
                }
                Divider()
                HStack {
                    Text("Connected Peers")
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text("\(sdkManager.ble.connectedPeers.count)")
                        .fontWeight(.semibold)
                }
                if !sdkManager.ble.connectedPeers.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(sdkManager.ble.connectedPeers, id: \.self) { peer in
                            Text("• \(peer.prefix(18))…")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .padding(.top, 4)
        }
    }
}

// MARK: - Queue Metrics Card

private struct QueueMetricsCard: View {
    @EnvironmentObject var sdkManager: SdkManager

    var body: some View {
        Card(title: "Queue Metrics") {
            if let m = sdkManager.metrics {
                VStack(spacing: 6) {
                    MetricRow("Outbound total",   value: m.outboundSize)
                    MetricRow("  High priority",  value: m.outboundHighPriority)
                    MetricRow("  Normal priority",value: m.outboundNormalPriority)
                    MetricRow("  Low priority",   value: m.outboundLowPriority)
                    Divider()
                    MetricRow("Confirmations",    value: m.confirmationSize)
                    MetricRow("Retry queue",      value: m.retrySize)
                    HStack {
                        Text("Avg retry attempts")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text(String(format: "%.1f", m.retryAvgAttempts))
                            .fontWeight(.medium)
                    }
                    .font(.subheadline)
                }
                .padding(.top, 4)
            } else {
                Text("Waiting for data…")
                    .foregroundStyle(.secondary)
                    .padding(.top, 4)
            }
        }
    }
}

private struct MetricRow: View {
    let label: String
    let value: Int
    init(_ label: String, value: Int) { self.label = label; self.value = value }

    var body: some View {
        HStack {
            Text(label).foregroundStyle(.secondary)
            Spacer()
            Text("\(value)").fontWeight(.medium)
        }
        .font(.subheadline)
    }
}

// MARK: - Reusable primitives

struct Card<Content: View>: View {
    let title: String
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.headline)
                .padding(.bottom, 4)
            content()
        }
        .padding()
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .shadow(color: .black.opacity(0.07), radius: 4, y: 2)
    }
}

struct StatusDot: View {
    var active: Bool
    var color: Color = .green

    var body: some View {
        Circle()
            .fill(active ? color : Color.gray.opacity(0.4))
            .frame(width: 10, height: 10)
    }
}
