import Foundation
import PolliNetFFI

// MARK: - Configuration

/// Configuration passed to the Rust core on initialisation.
public struct SdkConfig: Encodable {
    public var version: Int = 1
    public var rpcUrl: String?
    public var enableLogging: Bool = true
    public var logLevel: String? = "info"
    public var storageDirectory: String?
    public var encryptionKey: String?
    /// Base58-encoded Solana wallet address used for reward attribution.
    public var walletAddress: String?

    public init(
        rpcUrl: String? = nil,
        enableLogging: Bool = true,
        logLevel: String? = "info",
        storageDirectory: String? = nil,
        encryptionKey: String? = nil,
        walletAddress: String? = nil
    ) {
        self.rpcUrl = rpcUrl
        self.enableLogging = enableLogging
        self.logLevel = logLevel
        self.storageDirectory = storageDirectory
        self.encryptionKey = encryptionKey
        self.walletAddress = walletAddress
    }

    enum CodingKeys: String, CodingKey {
        case version, rpcUrl, enableLogging, logLevel, storageDirectory, encryptionKey, walletAddress
    }
}

// MARK: - SDK

/// High-level Swift wrapper over the PolliNet Rust core.
/// All methods are `async` — they dispatch blocking FFI calls off the main thread.
public actor PolliNetSDK {

    private let handle: Int64

    private init(handle: Int64) {
        self.handle = handle
    }

    // MARK: Lifecycle

    /// Initialise a new SDK instance.
    public static func initialize(config: SdkConfig) async throws -> PolliNetSDK {
        let json = try JSONEncoder().encode(config)
        let jsonStr = String(data: json, encoding: .utf8)!
        let handle: Int64 = await Task.detached(priority: .userInitiated) {
            jsonStr.withCString { ptr in
                pollinet_init(ptr)
            }
        }.value
        guard handle >= 0 else {
            throw PolliNetError.initFailed("Rust returned handle -1")
        }
        return PolliNetSDK(handle: handle)
    }

    /// Return the compiled SDK version string.
    public static func version() -> String {
        guard let ptr = pollinet_version() else { return "unknown" }
        defer { pollinet_free_string(ptr) }
        return String(cString: ptr)
    }

    /// Shut down this SDK instance and release all Rust resources.
    public func shutdown() {
        pollinet_shutdown(handle)
    }

    // MARK: Transport

    /// Push raw bytes received from a Core Bluetooth characteristic into the Rust state machine.
    public func pushInbound(_ data: Data) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .userInitiated) {
                let result: String = data.withUnsafeBytes { buf in
                    let ptr = buf.baseAddress!.assumingMemoryBound(to: UInt8.self)
                    let raw = pollinet_push_inbound(self.handle, ptr, buf.count)!
                    defer { pollinet_free_string(raw) }
                    return String(cString: raw)
                }
                do {
                    try Self.checkResult(result)
                    cont.resume()
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    /// Get the next outbound BLE frame. Returns `nil` when the queue is empty.
    public func nextOutbound(maxLen: Int = 244) async -> Data? {
        await Task.detached(priority: .userInitiated) {
            var outLen: Int = 0
            guard let ptr = pollinet_next_outbound(self.handle, maxLen, &outLen), outLen > 0 else {
                return nil
            }
            defer { pollinet_free_bytes(ptr, outLen) }
            return Data(bytes: ptr, count: outLen)
        }.value
    }

    /// Advance protocol timers (retry back-off, expiry). Call periodically.
    public func tick() async throws {
        try await ffiVoid { pollinet_tick(self.handle, Int64(Date().timeIntervalSince1970 * 1000)) }
    }

    // MARK: Received queue

    /// Pop the next reassembled transaction ready for RPC submission.
    public func nextReceivedTransaction() async throws -> ReceivedTransaction? {
        try await ffiDecode(ReceivedTransaction?.self) {
            pollinet_next_received_transaction(self.handle)
        }
    }

    /// Push a reassembled transaction into the received queue.
    public func pushReceivedTransaction(_ data: Data) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .userInitiated) {
                let result: String = data.withUnsafeBytes { buf in
                    let ptr = buf.baseAddress!.assumingMemoryBound(to: UInt8.self)
                    let raw = pollinet_push_received_transaction(self.handle, ptr, buf.count)!
                    defer { pollinet_free_string(raw) }
                    return String(cString: raw)
                }
                do {
                    try Self.checkResult(result)
                    cont.resume()
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    // MARK: Submission

    /// Submit a transaction to the Solana RPC. Returns the on-chain signature.
    public func submitOfflineTransaction(transactionBase64: String, verifyNonce: Bool = false) async throws -> String {
        let req = SubmitRequest(transactionBase64: transactionBase64, verifyNonce: verifyNonce)
        let reqJson = try JSONEncoder().encode(req)
        let reqStr = String(data: reqJson, encoding: .utf8)!
        return try await ffiDecode(String.self) {
            reqStr.withCString { pollinet_submit_offline_transaction(self.handle, $0) }
        }
    }

    /// Mark a transaction as submitted (deduplication — prevents re-submission).
    public func markTransactionSubmitted(_ data: Data) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .userInitiated) {
                let result: String = data.withUnsafeBytes { buf in
                    let ptr = buf.baseAddress!.assumingMemoryBound(to: UInt8.self)
                    let raw = pollinet_mark_transaction_submitted(self.handle, ptr, buf.count)!
                    defer { pollinet_free_string(raw) }
                    return String(cString: raw)
                }
                do {
                    try Self.checkResult(result)
                    cont.resume()
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    // MARK: Retry queue

    /// Add a failed transaction to the exponential back-off retry queue.
    public func addToRetryQueue(txBytes: Data, txId: String, error: String) async throws {
        let req = AddToRetryRequest(txBytes: txBytes.base64EncodedString(), txId: txId, error: error)
        let reqJson = try JSONEncoder().encode(req)
        let reqStr = String(data: reqJson, encoding: .utf8)!
        try await ffiVoid { reqStr.withCString { pollinet_add_to_retry_queue(self.handle, $0) } }
    }

    /// Pop the next retry item whose back-off timer has expired.
    public func popReadyRetry() async throws -> RetryItem? {
        try await ffiDecode(RetryItem?.self) { pollinet_pop_ready_retry(self.handle) }
    }

    // MARK: Confirmation queue

    /// Queue a SUCCESS confirmation for relay back to the origin node.
    public func queueConfirmation(txId: String, signature: String) async throws {
        let req = QueueConfirmationRequest(txId: txId, signature: signature)
        let reqJson = try JSONEncoder().encode(req)
        let reqStr = String(data: reqJson, encoding: .utf8)!
        try await ffiVoid { reqStr.withCString { pollinet_queue_confirmation(self.handle, $0) } }
    }

    /// Queue a FAILURE confirmation for relay back to the origin node.
    public func queueFailureConfirmation(txId: String, error: String) async throws {
        let conf = Confirmation(txId: txId, status: .failed(error: error),
                                timestamp: Int64(Date().timeIntervalSince1970 * 1000), relayCount: 0)
        try await relayConfirmation(conf)
    }

    /// Pop the next outbound confirmation.
    public func popConfirmation() async throws -> Confirmation? {
        try await ffiDecode(Confirmation?.self) { pollinet_pop_confirmation(self.handle) }
    }

    /// Relay a received confirmation (increments hop count, re-queues for relay).
    public func relayConfirmation(_ confirmation: Confirmation) async throws {
        let json = try JSONEncoder().encode(confirmation)
        let jsonStr = String(data: json, encoding: .utf8)!
        try await ffiVoid { jsonStr.withCString { pollinet_relay_confirmation(self.handle, $0) } }
    }

    // MARK: Fragmentation

    /// Fragment a signed transaction into BLE-sized chunks.
    public func fragmentTransaction(_ data: Data, maxPayload: Int = 0) async throws -> FragmentList {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<FragmentList, Error>) in
            Task.detached(priority: .userInitiated) {
                let result: String = data.withUnsafeBytes { buf in
                    let ptr = buf.baseAddress!.assumingMemoryBound(to: UInt8.self)
                    let raw = pollinet_fragment_transaction(self.handle, ptr, buf.count, maxPayload)!
                    defer { pollinet_free_string(raw) }
                    return String(cString: raw)
                }
                do {
                    let list = try Self.decodeResult(FragmentList.self, from: result)
                    cont.resume(returning: list)
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    // MARK: Cleanup

    /// Remove stale fragments from the reassembly buffer.
    public func cleanupStaleFragments() async throws {
        try await ffiVoid { pollinet_cleanup_stale_fragments(self.handle) }
    }

    /// Remove expired retry items and confirmations.
    public func cleanupExpired() async throws {
        try await ffiVoid { pollinet_cleanup_expired(self.handle) }
    }

    // MARK: Metrics

    /// Return current queue size metrics.
    public func getQueueMetrics() async throws -> QueueMetrics {
        try await ffiDecode(QueueMetrics.self) { pollinet_get_queue_metrics(self.handle) }
    }

    // MARK: Wallet address

    /// Update the wallet address for reward attribution. Pass `nil` to clear.
    public func setWalletAddress(_ address: String?) async throws {
        let addr = address ?? ""
        try await ffiVoid { addr.withCString { pollinet_set_wallet_address(self.handle, $0) } }
    }

    /// Return the wallet address currently stored in the Rust transport, or `nil`.
    public func getWalletAddress() async throws -> String? {
        let resp = try await ffiDecode(WalletAddressResponse.self) {
            pollinet_get_wallet_address(self.handle)
        }
        return resp.address.isEmpty ? nil : resp.address
    }

    // MARK: - Private FFI helpers

    /// Call an FFI function that returns a JSON result and expect a Void response.
    private func ffiVoid(_ call: @escaping () -> UnsafeMutablePointer<CChar>?) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .userInitiated) {
                guard let raw = call() else {
                    cont.resume(throwing: PolliNetError.nullResponse)
                    return
                }
                defer { pollinet_free_string(raw) }
                let json = String(cString: raw)
                do {
                    try Self.checkResult(json)
                    cont.resume()
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    /// Call an FFI function that returns a JSON result and decode the `data` field as `T`.
    private func ffiDecode<T: Decodable>(_ type: T.Type, _ call: @escaping () -> UnsafeMutablePointer<CChar>?) async throws -> T {
        try await withCheckedThrowingContinuation { cont in
            Task.detached(priority: .userInitiated) {
                guard let raw = call() else {
                    cont.resume(throwing: PolliNetError.nullResponse)
                    return
                }
                defer { pollinet_free_string(raw) }
                let json = String(cString: raw)
                do {
                    let value = try Self.decodeResult(type, from: json)
                    cont.resume(returning: value)
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    private static func checkResult(_ json: String) throws {
        let data = json.data(using: .utf8)!
        let envelope = try JSONDecoder().decode(FfiEnvelope.self, from: data)
        if !envelope.ok {
            throw PolliNetError.ffiError(code: envelope.code ?? "UNKNOWN",
                                         message: envelope.message ?? json)
        }
    }

    private static func decodeResult<T: Decodable>(_ type: T.Type, from json: String) throws -> T {
        let data = json.data(using: .utf8)!
        let wrapper = try JSONDecoder().decode(FfiResult<T>.self, from: data)
        if !wrapper.ok {
            throw PolliNetError.ffiError(code: wrapper.code ?? "UNKNOWN",
                                         message: wrapper.message ?? json)
        }
        guard let value = wrapper.data else {
            throw PolliNetError.missingData
        }
        return value
    }
}

// MARK: - Errors

public enum PolliNetError: Error, LocalizedError {
    case initFailed(String)
    case ffiError(code: String, message: String)
    case nullResponse
    case missingData

    public var errorDescription: String? {
        switch self {
        case .initFailed(let msg):         return "SDK init failed: \(msg)"
        case .ffiError(let code, let msg): return "[\(code)] \(msg)"
        case .nullResponse:                return "FFI returned null pointer"
        case .missingData:                 return "FFI result missing data field"
        }
    }
}

// MARK: - Internal envelope types (decode FFI JSON)

private struct FfiEnvelope: Decodable {
    let ok: Bool
    let code: String?
    let message: String?
}

private struct FfiResult<T: Decodable>: Decodable {
    let ok: Bool
    let data: T?
    let code: String?
    let message: String?
}

// MARK: - Internal request types

private struct SubmitRequest: Encodable {
    let transactionBase64: String
    let verifyNonce: Bool
}

private struct AddToRetryRequest: Encodable {
    let txBytes: String
    let txId: String
    let error: String
}

private struct QueueConfirmationRequest: Encodable {
    let txId: String
    let signature: String
}

private struct WalletAddressResponse: Decodable {
    let address: String
}
