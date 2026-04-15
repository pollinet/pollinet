import SwiftUI

struct LogsView: View {
    @EnvironmentObject var sdkManager: SdkManager
    @State private var filter: LogEntry.Level? = nil
    @State private var searchText = ""

    private var filteredLogs: [LogEntry] {
        sdkManager.logs
            .filter { filter == nil || $0.level == filter }
            .filter { searchText.isEmpty || $0.message.localizedCaseInsensitiveContains(searchText) }
    }

    var body: some View {
        NavigationView {
            VStack(spacing: 0) {
                FilterBar(selected: $filter)
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                    .background(.background)

                Divider()

                if filteredLogs.isEmpty {
                    Spacer()
                    Text("No log entries")
                        .foregroundStyle(.secondary)
                    Spacer()
                } else {
                    ScrollViewReader { proxy in
                        List(filteredLogs.reversed()) { entry in
                            LogRow(entry: entry)
                                .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
                                .id(entry.id)
                        }
                        .listStyle(.plain)
                        .onChange(of: sdkManager.logs.count) { _ in
                            if let last = filteredLogs.last {
                                proxy.scrollTo(last.id, anchor: .bottom)
                            }
                        }
                    }
                }
            }
            .searchable(text: $searchText, prompt: "Search logs")
            .navigationTitle("Logs")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Clear") { sdkManager.logs.removeAll() }
                        .foregroundStyle(.red)
                }
            }
        }
    }
}

// MARK: - Filter bar

private struct FilterBar: View {
    @Binding var selected: LogEntry.Level?

    private let levels: [(LogEntry.Level?, String, Color)] = [
        (nil,      "All",     .primary),
        (.info,    "Info",    .blue),
        (.success, "OK",      .green),
        (.warning, "Warn",    .orange),
        (.error,   "Error",   .red),
    ]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(levels, id: \.1) { level, label, color in
                    Button(label) { selected = level }
                        .buttonStyle(FilterChipStyle(active: selected == level, color: color))
                }
            }
        }
    }
}

private struct FilterChipStyle: ButtonStyle {
    let active: Bool
    let color: Color

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(active ? color.opacity(0.15) : Color(.systemGray6))
            .foregroundStyle(active ? color : .secondary)
            .clipShape(Capsule())
            .overlay(Capsule().stroke(active ? color.opacity(0.5) : Color.clear, lineWidth: 1))
    }
}

// MARK: - Log row

private struct LogRow: View {
    let entry: LogEntry

    var levelColor: Color {
        switch entry.level {
        case .info:    return .blue
        case .success: return .green
        case .warning: return .orange
        case .error:   return .red
        }
    }

    var levelIcon: String {
        switch entry.level {
        case .info:    return "info.circle"
        case .success: return "checkmark.circle"
        case .warning: return "exclamationmark.triangle"
        case .error:   return "xmark.circle"
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: levelIcon)
                .foregroundStyle(levelColor)
                .frame(width: 16)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.message)
                    .font(.caption)
                    .fixedSize(horizontal: false, vertical: true)
                Text(entry.timeString)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
    }
}
