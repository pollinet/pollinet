import Foundation
import Combine
import PolliNetSDK

// MARK: - Log entry

struct LogEntry: Identifiable {
    let id = UUID()
    let timestamp: Date
    let message: String
    let level: Level

    enum Level { case info, success, warning, error }

    var timeString: String {
        let f = DateFormatter(); f.dateFormat = "HH:mm:ss"
        return f.string(from: timestamp)
    }
}

// MARK: - SdkManager

@MainActor
final class SdkManager: NSObject, ObservableObject {

    // MARK: Published state
    @Published private(set) var isInitialized = false
    @Published private(set) var sdkVersion    = ""
    @Published private(set) var metrics: QueueMetrics?
    @Published private(set) var logs: [LogEntry] = []
    @Published var walletAddress: String = ""

    // MARK: Sub-managers
    let ble = BleManager()

    // MARK: Private
    private var sdk: PolliNetSDK?
    private var tickTask: Task<Void, Never>?
    private var metricsTask: Task<Void, Never>?

    // MARK: Init

    override init() {
        super.init()
        ble.delegate = self
        Task { await initializeSdk() }
    }

    // MARK: SDK lifecycle

    func initializeSdk() async {
        sdkVersion = PolliNetSDK.version()
        let docsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].path
        let config = SdkConfig(
            enableLogging: true,
            logLevel: "info",
            storageDirectory: docsDir,
            walletAddress: walletAddress.isEmpty ? nil : walletAddress
        )
        do {
            sdk = try await PolliNetSDK.initialize(config: config)
            isInitialized = true
            appendLog("SDK initialized (v\(sdkVersion))", level: .success)
            startTickLoop()
            startMetricsLoop()
        } catch {
            appendLog("SDK init failed: \(error.localizedDescription)", level: .error)
        }
    }

    func shutdown() {
        tickTask?.cancel()
        metricsTask?.cancel()
        sdk?.shutdown()
        sdk = nil
        isInitialized = false
        appendLog("SDK shut down", level: .warning)
    }

    // MARK: Wallet address

    func applyWalletAddress() async {
        guard let sdk else { return }
        do {
            try await sdk.setWalletAddress(walletAddress.isEmpty ? nil : walletAddress)
            appendLog("Wallet address set: \(walletAddress)", level: .success)
        } catch {
            appendLog("Set wallet address failed: \(error.localizedDescription)", level: .error)
        }
    }

    // MARK: Received-queue processing

    /// Drain the received-transaction queue. Submit each tx to Solana RPC.
    func processReceivedQueue() async {
        guard let sdk else { return }
        while true {
            do {
                guard let tx = try await sdk.nextReceivedTransaction() else { break }
                appendLog("TX received: \(tx.txId.prefix(12))…", level: .info)
                await submitTransaction(tx)
            } catch {
                appendLog("Received queue error: \(error.localizedDescription)", level: .error)
                break
            }
        }
    }

    private func submitTransaction(_ tx: ReceivedTransaction) async {
        guard let sdk else { return }
        do {
            let sig = try await sdk.submitOfflineTransaction(transactionBase64: tx.transactionBase64)
            appendLog("TX submitted ✓ sig=\(sig.prefix(16))…", level: .success)
            try await sdk.queueConfirmation(txId: tx.txId, signature: sig)
            appendLog("Confirmation queued for \(tx.txId.prefix(12))…", level: .info)
            await drainConfirmationQueue()
        } catch {
            appendLog("Submit failed [\(tx.txId.prefix(12))]: \(error.localizedDescription)", level: .warning)
            do {
                try await sdk.addToRetryQueue(
                    txBytes: Data(base64Encoded: tx.transactionBase64) ?? Data(),
                    txId: tx.txId,
                    error: error.localizedDescription
                )
                appendLog("TX added to retry queue", level: .info)
            } catch {
                appendLog("Retry queue error: \(error.localizedDescription)", level: .error)
            }
            do {
                try await sdk.queueFailureConfirmation(txId: tx.txId, error: error.localizedDescription)
                await drainConfirmationQueue()
            } catch {
                appendLog("Failure confirmation error: \(error.localizedDescription)", level: .error)
            }
        }
    }

    // MARK: Retry-queue processing

    func processRetryQueue() async {
        guard let sdk else { return }
        while true {
            do {
                guard let item = try await sdk.popReadyRetry() else { break }
                guard let txBytes = Data(base64Encoded: item.txBytes) else {
                    appendLog("Bad base64 in retry item \(item.txId.prefix(12))", level: .error)
                    continue
                }
                appendLog("Retrying TX \(item.txId.prefix(12)) (attempt \(item.attemptCount + 1))", level: .info)
                await retryTransaction(txBytes: txBytes, item: item)
            } catch {
                appendLog("Retry queue error: \(error.localizedDescription)", level: .error)
                break
            }
        }
    }

    private func retryTransaction(txBytes: Data, item: RetryItem) async {
        guard let sdk else { return }
        let b64 = txBytes.base64EncodedString()
        do {
            let sig = try await sdk.submitOfflineTransaction(transactionBase64: b64)
            appendLog("Retry success ✓ sig=\(sig.prefix(16))…", level: .success)
            try await sdk.markTransactionSubmitted(txBytes)
            try await sdk.queueConfirmation(txId: item.txId, signature: sig)
            await drainConfirmationQueue()
        } catch {
            if item.attemptCount + 1 >= 5 {
                appendLog("Max retries reached for \(item.txId.prefix(12))", level: .warning)
                do {
                    try await sdk.queueFailureConfirmation(
                        txId: item.txId,
                        error: "Max retries (5) exceeded: \(error.localizedDescription)"
                    )
                    await drainConfirmationQueue()
                } catch {
                    appendLog("Failure confirmation error: \(error.localizedDescription)", level: .error)
                }
            } else {
                do {
                    try await sdk.addToRetryQueue(txBytes: txBytes, txId: item.txId,
                                                  error: error.localizedDescription)
                } catch {
                    appendLog("Re-queue error: \(error.localizedDescription)", level: .error)
                }
            }
        }
    }

    // MARK: Confirmation queue

    func drainConfirmationQueue() async {
        guard let sdk else { return }
        while true {
            do {
                guard let conf = try await sdk.popConfirmation() else { break }
                let payload = try JSONEncoder().encode(conf)
                ble.send(payload)
                appendLog("Confirmation relayed: \(conf.txId.prefix(12))…", level: .info)
            } catch {
                appendLog("Confirmation drain error: \(error.localizedDescription)", level: .error)
                break
            }
        }
    }

    // MARK: Fragmentation

    /// Fragment a raw signed transaction and push each fragment into the outbound BLE queue.
    func enqueueTransaction(_ txData: Data) async {
        guard let sdk else { return }
        do {
            let list = try await sdk.fragmentTransaction(txData)
            for fragment in list.fragments {
                guard let payload = fragment.data.data(using: .utf8) else { continue }
                try await sdk.pushInbound(payload)
            }
            appendLog("TX fragmented into \(list.fragments.count) chunks", level: .info)
        } catch {
            appendLog("Fragment error: \(error.localizedDescription)", level: .error)
        }
    }

    // MARK: Cleanup

    func runCleanup() async {
        guard let sdk else { return }
        do {
            try await sdk.cleanupStaleFragments()
            try await sdk.cleanupExpired()
            appendLog("Cleanup completed", level: .info)
        } catch {
            appendLog("Cleanup error: \(error.localizedDescription)", level: .error)
        }
    }

    // MARK: Tick loop

    private func startTickLoop() {
        tickTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 500_000_000) // 500 ms
                guard let self, let sdk = self.sdk else { continue }
                do {
                    try await sdk.tick()
                    await self.processRetryQueue()
                } catch {
                    await self.appendLog("Tick error: \(error.localizedDescription)", level: .error)
                }
            }
        }
    }

    // MARK: Metrics loop

    private func startMetricsLoop() {
        metricsTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 3_000_000_000) // 3 s
                guard let self, let sdk = self.sdk else { continue }
                do {
                    let m = try await sdk.getQueueMetrics()
                    await MainActor.run { self.metrics = m }
                } catch {}
            }
        }
    }

    // MARK: Drain outbound BLE queue

    private func drainOutboundQueue() async {
        guard let sdk else { return }
        while true {
            guard let frame = await sdk.nextOutbound() else { break }
            ble.send(frame)
        }
    }

    // MARK: Logging helper

    func appendLog(_ message: String, level: LogEntry.Level = .info) {
        let entry = LogEntry(timestamp: .now, message: message, level: level)
        logs.append(entry)
        if logs.count > 500 { logs.removeFirst(logs.count - 500) }
        print("[SdkManager] \(message)")
    }
}

// MARK: - BleManagerDelegate

extension SdkManager: BleManagerDelegate {

    func bleManager(_ manager: BleManager, didReceiveFrame data: Data) {
        guard let sdk else { return }
        Task {
            do {
                try await sdk.pushInbound(data)
                await processReceivedQueue()
                await drainOutboundQueue()
            } catch {
                appendLog("Push inbound error: \(error.localizedDescription)", level: .error)
            }
        }
    }

    func bleManagerDidConnect(_ manager: BleManager) {
        Task { await drainConfirmationQueue() }
    }

    func bleManagerDidDisconnect(_ manager: BleManager) {
        appendLog("Peer disconnected", level: .warning)
    }
}
