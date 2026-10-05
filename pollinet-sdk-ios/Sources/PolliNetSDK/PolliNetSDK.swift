//
//  PolliNetSDK.swift
//  High-level PolliNet SDK for iOS — the Swift counterpart of Android's
//  PolliNetSDK.kt, method-for-method. Wraps the Rust core via the C ABI
//  (PolliNetRust.xcframework); async methods run the blocking FFI calls off
//  the caller's actor.
//

import Foundation
import PolliNetRust

public final class PolliNetSDK: @unchecked Sendable {

    // =========================================================================
    // Companion / lifecycle
    // =========================================================================

    /// Gateway gas-fee take rate in basis points (10 bps = 0.10%), applied by
    /// `createIntentBytes`. Must match the executor program's expectations.
    public static let gasFeeTakeRateBps: Int64 = 10

    /// Handle into the Rust transport registry.
    public let transportHandle: Int64

    private let rpcUrl: String?
    private let stateLock = NSLock()
    private var cachedGatewayWallet: String?
    private var cachedExecutorPda: String?

    private init(handle: Int64, rpcUrl: String?) {
        self.transportHandle = handle
        self.rpcUrl = rpcUrl
    }

    /// Initialize the SDK with a BLE-backed engine.
    public static func initialize(config: SdkConfig) async throws -> PolliNetSDK {
        try await FFI.run {
            let handle = try FFI.withJSON(config) { pollinet_init($0) }
            guard handle >= 0 else {
                throw PolliNetError(code: "ERR_INIT", message: "SDK initialization failed (see native log)")
            }
            return PolliNetSDK(handle: handle, rpcUrl: config.rpcUrl)
        }
    }

    /// Initialize a standalone Wi-Fi/Multipeer engine (larger default MTU).
    /// BLE-specific calls reject this handle by design.
    public static func initializeWifiDirect(config: SdkConfig) async throws -> PolliNetSDK {
        try await FFI.run {
            let handle = try FFI.withJSON(config) { pollinet_init_wifi_direct($0) }
            guard handle >= 0 else {
                throw PolliNetError(code: "ERR_INIT", message: "Wi-Fi transport initialization failed (see native log)")
            }
            return PolliNetSDK(handle: handle, rpcUrl: config.rpcUrl)
        }
    }

    /// SDK version (from the Rust core).
    public static func version() -> String {
        FFI.consume(pollinet_version())
    }

    /// Transport kind for this handle: "BLE" | "WIFI_DIRECT" ("" if invalid).
    public func transportKind() -> String {
        FFI.consume(pollinet_transport_kind(transportHandle))
    }

    /// Create a Multipeer/Wi-Fi handle that SHARES this BLE engine — one dedup
    /// set, outbound queue, and received queue across both radios.
    /// Returns the new handle, or -1 on failure.
    public func createSharedWifiDirectHandle() -> Int64 {
        pollinet_init_wifi_direct_sharing(transportHandle)
    }

    /// SDK bound to a Wi-Fi/Multipeer handle sharing this engine (cross-transport
    /// dedup). Use for `MultipeerController`; nil if the shared init failed.
    public func makeSharedWifiDirectSDK() -> PolliNetSDK? {
        let handle = createSharedWifiDirectHandle()
        guard handle >= 0 else { return nil }
        return PolliNetSDK(handle: handle, rpcUrl: rpcUrl)
    }

    /// Shut down this handle and release its resources.
    public func shutdown() {
        pollinet_shutdown(transportHandle)
    }

    // =========================================================================
    // Transport API (host-driven byte pump)
    // =========================================================================

    /// Push inbound bytes received from the radio into the Rust engine.
    public func pushInbound(_ data: Data) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: FFI.withBytes(data) { ptr, len in
                FFI.consume(pollinet_push_inbound(handle, ptr, len))
            })
        }
    }

    /// Next outbound frame to send (at most `maxLen` bytes), or nil when idle.
    public func nextOutbound(maxLen: Int = 1024) async -> Data? {
        let handle = transportHandle
        return try? await FFI.run {
            var outLen: UInt = 0
            guard let ptr = pollinet_next_outbound(handle, UInt(maxLen), &outLen), outLen > 0 else {
                return nil as Data?
            }
            defer { pollinet_bytes_free(ptr, outLen) }
            return Data(bytes: ptr, count: Int(outLen))
        } ?? nil
    }

    /// Periodic tick for retry/timeout handling. Returns frames to (re)send as
    /// base64 strings.
    public func tick() async throws -> [String] {
        let handle = transportHandle
        return try await FFI.run {
            let now = UInt64(Date().timeIntervalSince1970 * 1000)
            return try FFI.decode([String].self, from: FFI.consume(pollinet_tick(handle, now)))
        }
    }

    /// Current transport metrics.
    public func metrics() async throws -> MetricsSnapshot {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(MetricsSnapshot.self, from: FFI.consume(pollinet_metrics(handle)))
        }
    }

    /// Clear a transaction from reassembly buffers.
    public func clearTransaction(txId: String) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: txId.withCString { FFI.consume(pollinet_clear_transaction(handle, $0)) })
        }
    }

    // =========================================================================
    // Fragmentation
    // =========================================================================

    /// Fragment a transaction for radio transmission (also queues it outbound).
    public func fragment(txBytes: Data, maxPayload: Int? = nil) async throws -> FragmentList {
        let handle = transportHandle
        return try await FFI.run {
            let raw = FFI.withBytes(txBytes) { ptr, len in
                FFI.consume(pollinet_fragment(handle, ptr, len, Int64(maxPayload ?? 0)))
            }
            return try FFI.decode(FragmentList.self, from: raw)
        }
    }

    /// Reconstruct a transaction (base64) from received fragments.
    public func reconstructTransaction(fragments: [FragmentData]) async throws -> String {
        try await FFI.run {
            let raw = try FFI.withJSON(fragments) { FFI.consume(pollinet_reconstruct_transaction($0)) }
            return try FFI.decode(String.self, from: raw)
        }
    }

    /// Fragmentation statistics for a transaction (no queueing).
    public func getFragmentationStats(transactionBytes: Data) async throws -> FragmentationStats {
        try await FFI.run {
            let raw = FFI.withBytes(transactionBytes) { ptr, len in
                FFI.consume(pollinet_get_fragmentation_stats(ptr, len))
            }
            return try FFI.decode(FragmentationStats.self, from: raw)
        }
    }

    /// Prepare a broadcast: fragments + ready-to-send mesh packets.
    public func prepareBroadcast(transactionBytes: Data) async throws -> BroadcastPreparation {
        let handle = transportHandle
        return try await FFI.run {
            let raw = FFI.withBytes(transactionBytes) { ptr, len in
                FFI.consume(pollinet_prepare_broadcast(handle, ptr, len))
            }
            return try FFI.decode(BroadcastPreparation.self, from: raw)
        }
    }

    // =========================================================================
    // Autonomous relay / received queue
    // =========================================================================

    /// Push a fully-reassembled transaction into the auto-submission queue.
    public func pushReceivedTransaction(transactionBytes: Data) async throws -> PushResponse {
        let handle = transportHandle
        return try await FFI.run {
            let raw = FFI.withBytes(transactionBytes) { ptr, len in
                FFI.consume(pollinet_push_received_transaction(handle, ptr, len))
            }
            return try FFI.decode(PushResponse.self, from: raw)
        }
    }

    /// Pop the next received transaction for submission, or nil when empty.
    public func nextReceivedTransaction() async throws -> ReceivedTransaction? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(
                ReceivedTransaction.self,
                from: FFI.consume(pollinet_next_received_transaction(handle))
            )
        }
    }

    public func getReceivedQueueSize() async throws -> Int {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(QueueSizeResponse.self, from: FFI.consume(pollinet_get_received_queue_size(handle))).queueSize
        }
    }

    /// Reassembly progress for all incomplete inbound transactions.
    public func getFragmentReassemblyInfo() async throws -> FragmentReassemblyInfoList {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(
                FragmentReassemblyInfoList.self,
                from: FFI.consume(pollinet_get_fragment_reassembly_info(handle))
            )
        }
    }

    /// Mark a transaction as submitted (dedup across the mesh).
    @discardableResult
    public func markTransactionSubmitted(transactionBytes: Data) async throws -> Bool {
        let handle = transportHandle
        return try await FFI.run {
            let raw = FFI.withBytes(transactionBytes) { ptr, len in
                FFI.consume(pollinet_mark_transaction_submitted(handle, ptr, len))
            }
            return try FFI.decode(SuccessResponse.self, from: raw).success
        }
    }

    @discardableResult
    public func cleanupOldSubmissions() async throws -> Bool {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(SuccessResponse.self, from: FFI.consume(pollinet_cleanup_old_submissions(handle))).success
        }
    }

    /// Non-destructive dump of the outbound fragment queue.
    public func debugOutboundQueue() async throws -> OutboundQueueDebug {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(OutboundQueueDebug.self, from: FFI.consume(pollinet_debug_outbound_queue(handle)))
        }
    }

    // =========================================================================
    // Queue management
    // =========================================================================

    /// Push a pre-fragmented transaction into the outbound priority queue.
    public func pushOutboundTransaction(
        txBytes: Data,
        txId: String,
        fragments: [FragmentFFI],
        priority: Priority = .normal
    ) async throws {
        let handle = transportHandle
        let request = PushOutboundRequest(
            txBytes: txBytes.base64EncodedString(),
            txId: txId,
            fragments: fragments,
            priority: priority
        )
        try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_push_outbound_transaction(handle, $0)) }
            try FFI.decodeVoid(from: raw)
        }
    }

    /// Verify, fragment, and queue an externally-signed transaction. Returns its tx id.
    public func acceptAndQueueExternalTransaction(
        base64SignedTx: String,
        maxPayload: Int? = nil
    ) async throws -> String {
        let handle = transportHandle
        let request = AcceptExternalTransactionRequest(base64SignedTx: base64SignedTx, maxPayload: maxPayload)
        return try await FFI.run {
            let raw = try FFI.withJSON(request) {
                FFI.consume(pollinet_accept_and_queue_external_transaction(handle, $0))
            }
            return try FFI.decode(String.self, from: raw)
        }
    }

    /// Pop the next outbound transaction (highest priority first), or nil when empty.
    public func popOutboundTransaction() async throws -> OutboundTransaction? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(
                OutboundTransaction.self,
                from: FFI.consume(pollinet_pop_outbound_transaction(handle))
            )
        }
    }

    public func getOutboundQueueSize() async throws -> Int {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(QueueSizeResponse.self, from: FFI.consume(pollinet_get_outbound_queue_size(handle))).queueSize
        }
    }

    /// Schedule a failed submission for retry with exponential backoff.
    public func addToRetryQueue(txBytes: Data, txId: String, error: String) async throws {
        let handle = transportHandle
        let request = AddToRetryRequest(txBytes: txBytes.base64EncodedString(), txId: txId, error: error)
        try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_add_to_retry_queue(handle, $0)) }
            try FFI.decodeVoid(from: raw)
        }
    }

    /// Pop the next retry item whose backoff has elapsed, or nil.
    public func popReadyRetry() async throws -> RetryItem? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(RetryItem.self, from: FFI.consume(pollinet_pop_ready_retry(handle)))
        }
    }

    public func getRetryQueueSize() async throws -> Int {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(QueueSizeResponse.self, from: FFI.consume(pollinet_get_retry_queue_size(handle))).queueSize
        }
    }

    /// Queue a success confirmation for relay back to the origin device.
    public func queueConfirmation(txId: String, signature: String) async throws {
        let handle = transportHandle
        let request = QueueConfirmationRequest(txId: txId, signature: signature)
        try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_queue_confirmation(handle, $0)) }
            try FFI.decodeVoid(from: raw)
        }
    }

    /// Queue a failure confirmation for relay back to the origin device.
    public func queueFailureConfirmation(txId: String, error: String) async throws {
        let confirmation = Confirmation(
            txId: txId,
            status: .failed(error: error),
            timestamp: Int64(Date().timeIntervalSince1970 * 1000),
            relayCount: 0
        )
        try await relayConfirmation(confirmation)
    }

    /// Pop the next confirmation, or nil when empty.
    public func popConfirmation() async throws -> Confirmation? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(Confirmation.self, from: FFI.consume(pollinet_pop_confirmation(handle)))
        }
    }

    /// Remove reassembly fragments older than 5 minutes. Returns fragments cleaned.
    @discardableResult
    public func cleanupStaleFragments() async throws -> Int {
        struct CleanupResponse: Codable {
            let fragmentsCleaned: Int
            enum CodingKeys: String, CodingKey { case fragmentsCleaned = "fragments_cleaned" }
        }
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(CleanupResponse.self, from: FFI.consume(pollinet_cleanup_stale_fragments(handle))).fragmentsCleaned
        }
    }

    /// Cleanup expired confirmations and retry items.
    /// Returns (confirmationsCleaned, retriesCleaned).
    @discardableResult
    public func cleanupExpired() async throws -> (confirmationsCleaned: Int, retriesCleaned: Int) {
        struct CleanupExpiredResponse: Codable {
            let confirmationsCleaned: Int
            let retriesCleaned: Int
            enum CodingKeys: String, CodingKey {
                case confirmationsCleaned = "confirmations_cleaned"
                case retriesCleaned = "retries_cleaned"
            }
        }
        let handle = transportHandle
        return try await FFI.run {
            let resp = try FFI.decode(CleanupExpiredResponse.self, from: FFI.consume(pollinet_cleanup_expired(handle)))
            return (resp.confirmationsCleaned, resp.retriesCleaned)
        }
    }

    /// Confirm fan-out delivery of `txId` to the current peer; true if evicted.
    @discardableResult
    public func confirmDelivered(txId: String) async throws -> Bool {
        struct RemovedResponse: Codable { let removed: Bool }
        let handle = transportHandle
        return try await FFI.run {
            let raw = txId.withCString { FFI.consume(pollinet_confirm_delivered(handle, $0)) }
            return try FFI.decode(RemovedResponse.self, from: raw).removed
        }
    }

    /// Load the highest-relevance outbound transaction's fragments into the send
    /// buffer. Returns its info, or nil when the queue is empty.
    public func loadForSending() async throws -> LoadForSendingResult? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(LoadForSendingResult.self, from: FFI.consume(pollinet_load_for_sending(handle)))
        }
    }

    /// Purge outbound transactions older than `maxAgeSecs`. Returns count removed.
    @discardableResult
    public func purgeStaleOutbound(maxAgeSecs: Int64 = 300) async throws -> Int {
        struct PurgeResponse: Codable { let removed: Int }
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(PurgeResponse.self, from: FFI.consume(pollinet_purge_stale_outbound(handle, maxAgeSecs))).removed
        }
    }

    // =========================================================================
    // Subsystem 1 — density-adaptive rotation
    // =========================================================================

    /// Record a scan observation (call on every discovered peer advertisement).
    public func recordScanResult(peerId: String) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: peerId.withCString { FFI.consume(pollinet_record_scan_result(handle, $0)) })
        }
    }

    /// Recompute adaptive session/cooldown parameters (call every ~10 s).
    public func getAdaptiveParams() async throws -> AdaptiveParams {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(AdaptiveParams.self, from: FFI.consume(pollinet_get_adaptive_params(handle)))
        }
    }

    /// Put `peerId` in cooldown for `cooldownMs` after a session ends.
    public func addPeerToCooldown(peerId: String, cooldownMs: Int64) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: peerId.withCString {
                FFI.consume(pollinet_add_peer_to_cooldown(handle, $0, UInt64(max(0, cooldownMs))))
            })
        }
    }

    public func isPeerInCooldown(peerId: String) async throws -> Bool {
        let handle = transportHandle
        return try await FFI.run {
            let raw = peerId.withCString { FFI.consume(pollinet_is_peer_in_cooldown(handle, $0)) }
            return try FFI.decode(Bool.self, from: raw)
        }
    }

    /// Sparse-network safety net: release the oldest cooldown entry early.
    /// Returns the released peer id, or nil if the list was empty.
    public func expireOldestCooldown() async throws -> String? {
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decodeOptional(String.self, from: FFI.consume(pollinet_expire_oldest_cooldown(handle)))
        }
    }

    /// Log a session telemetry record.
    public func logSessionTelemetry(record: SessionTelemetryRecord) async throws {
        let handle = transportHandle
        try await FFI.run {
            let raw = try FFI.withJSON(record) { FFI.consume(pollinet_log_session_telemetry(handle, $0)) }
            try FFI.decodeVoid(from: raw)
        }
    }

    // =========================================================================
    // Subsystem 2 — per-peer materialized queue
    // =========================================================================

    /// tx_ids that should be sent to `peerIdHex` (4-byte compact id, 8 hex chars).
    public func outboundForPeer(peerIdHex: String) async throws -> [String] {
        let handle = transportHandle
        return try await FFI.run {
            let raw = peerIdHex.withCString { FFI.consume(pollinet_outbound_for_peer(handle, $0)) }
            return try FFI.decode([String].self, from: raw)
        }
    }

    /// Drain-conditional delivery confirmation. Call ONLY on mutual drain.
    /// Returns true if the entry was evicted (relevance reached 0).
    @discardableResult
    public func confirmDeliveredByPeer(txId: String, peerIdHex: String) async throws -> Bool {
        struct RemovedResponse: Codable { let removed: Bool }
        let handle = transportHandle
        return try await FFI.run {
            let raw = txId.withCString { tx in
                peerIdHex.withCString { peer in
                    FFI.consume(pollinet_confirm_delivered_by_peer(handle, tx, peer))
                }
            }
            return try FFI.decode(RemovedResponse.self, from: raw).removed
        }
    }

    // =========================================================================
    // Subsystem 3 — confirmation-driven purge
    // =========================================================================

    /// Ingest a pollicore-signed MeshConfirmation frame: purge carrier entries,
    /// tombstone the tx, and re-queue the confirmation at HIGH priority.
    public func ingestConfirmation(confirmationBytes: Data) async throws -> IngestConfirmationResult {
        let handle = transportHandle
        return try await FFI.run {
            let raw = FFI.withBytes(confirmationBytes) { ptr, len in
                FFI.consume(pollinet_ingest_confirmation(handle, ptr, len))
            }
            return try FFI.decode(IngestConfirmationResult.self, from: raw)
        }
    }

    /// Whether `txIdHashHex` has an active tombstone (drop inbound fragments if so).
    public func isTombstoned(txIdHashHex: String) async throws -> Bool {
        struct TombResponse: Codable { let tombstoned: Bool }
        let handle = transportHandle
        return try await FFI.run {
            let raw = txIdHashHex.withCString { FFI.consume(pollinet_is_tombstoned(handle, $0)) }
            return try FFI.decode(TombResponse.self, from: raw).tombstoned
        }
    }

    /// Evict expired tombstones and cooldowns. Call from the periodic tick loop.
    public func periodicMaintenance() async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: FFI.consume(pollinet_periodic_maintenance(handle)))
        }
    }

    public func getTombstoneCount() async throws -> Int {
        struct CountResponse: Codable { let count: Int }
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(CountResponse.self, from: FFI.consume(pollinet_get_tombstone_count(handle))).count
        }
    }

    // =========================================================================
    // Queue persistence
    // =========================================================================

    /// Force-save all queues to disk.
    public func saveQueues() async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: FFI.consume(pollinet_save_queues(handle)))
        }
    }

    /// Debounced auto-save (no-op if nothing changed recently).
    public func autoSaveQueues() async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: FFI.consume(pollinet_auto_save_queues(handle)))
        }
    }

    /// Clear all queues and reassembly buffers (does NOT clear nonce data).
    public func clearAllQueues() async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: FFI.consume(pollinet_clear_all_queues(handle)))
        }
    }

    /// Re-queue a received confirmation for further relay (increments hop count).
    public func relayConfirmation(_ confirmation: Confirmation) async throws {
        let handle = transportHandle
        try await FFI.run {
            let raw = try FFI.withJSON(confirmation) { FFI.consume(pollinet_relay_confirmation(handle, $0)) }
            try FFI.decodeVoid(from: raw)
        }
    }

    // =========================================================================
    // Peer / mesh health
    // =========================================================================

    public func getHealthSnapshot() async throws -> HealthSnapshot {
        struct HealthSnapshotResponse: Codable { let snapshot: HealthSnapshot }
        let handle = transportHandle
        return try await FFI.run {
            try FFI.decode(HealthSnapshotResponse.self, from: FFI.consume(pollinet_get_health_snapshot(handle))).snapshot
        }
    }

    public func recordPeerHeartbeat(peerId: String) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: peerId.withCString { FFI.consume(pollinet_record_peer_heartbeat(handle, $0)) })
        }
    }

    public func recordPeerRssi(peerId: String, rssi: Int) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: peerId.withCString {
                FFI.consume(pollinet_record_peer_rssi(handle, $0, Int32(rssi)))
            })
        }
    }

    public func recordPeerLatency(peerId: String, latencyMs: Int) async throws {
        let handle = transportHandle
        try await FFI.run {
            try FFI.decodeVoid(from: peerId.withCString {
                FFI.consume(pollinet_record_peer_latency(handle, $0, UInt32(max(0, latencyMs))))
            })
        }
    }

    // =========================================================================
    // Wallet address — reward attribution
    // =========================================================================

    /// Set (or clear, with nil) the wallet address for this node session.
    public func setWalletAddress(_ address: String?) async throws {
        let handle = transportHandle
        let value = address ?? ""
        try await FFI.run {
            try FFI.decodeVoid(from: value.withCString { FFI.consume(pollinet_set_wallet_address(handle, $0)) })
        }
    }

    /// The wallet address currently set, or nil if none.
    public func getWalletAddress() async throws -> String? {
        let handle = transportHandle
        return try await FFI.run {
            let address = try FFI.decode(
                WalletAddressResponse.self,
                from: FFI.consume(pollinet_get_wallet_address(handle))
            ).address
            return address.isEmpty ? nil : address
        }
    }

    // =========================================================================
    // Intent protocol
    // =========================================================================

    /// Gateway (pollicore) wallet public key. Derive its ATA for the token mint
    /// to obtain the gas-fee payee account.
    public func getGatewayWallet() async throws -> String {
        let url = try pollicoreEndpoint("sdk/intents/gateway")
        let body = try await httpGet(url)
        return try JSONDecoder().decode(GatewayResponse.self, from: body).wallet
    }

    /// Deterministic, offline ATA derivation. The `to` of an intent must be a
    /// token account (ATA) — never a wallet address.
    public func deriveAssociatedTokenAccount(ownerWallet: String, tokenMint: String) async throws -> String {
        try await FFI.run {
            let ata = ownerWallet.withCString { owner in
                tokenMint.withCString { mint in
                    FFI.consume(pollinet_derive_associated_token_account(owner, mint))
                }
            }
            guard !ata.isEmpty else {
                throw PolliNetError(
                    code: "ERR_ATA",
                    message: "ATA derivation returned empty — check owner/mint are valid base58"
                )
            }
            return ata
        }
    }

    /// Executor PDA of the pollinet-executor program.
    public func getExecutorPda() async throws -> ExecutorPdaResponse {
        try await FFI.run {
            try FFI.decode(ExecutorPdaResponse.self, from: FFI.consume(pollinet_get_executor_pda()))
        }
    }

    /// Build the one-time unsigned `approve_checked` transaction delegating the
    /// executor PDA over each listed token account. Sign + submit before intents.
    public func createApproveTransaction(
        ownerWallet: String,
        tokens: [TokenApprovalEntry],
        feePayer: String? = nil,
        recentBlockhash: String
    ) async throws -> ApproveTransactionResponse {
        let request = CreateApproveTransactionRequest(
            ownerWallet: ownerWallet,
            feePayer: feePayer ?? ownerWallet,
            recentBlockhash: recentBlockhash,
            tokens: tokens
        )
        return try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_create_approve_transaction($0)) }
            return try FFI.decode(ApproveTransactionResponse.self, from: raw)
        }
    }

    /// Build the canonical 169-byte borsh Intent (base64) with its replay nonce.
    ///
    /// The gateway gas fee is filled in automatically:
    /// - payee = gateway wallet's ATA for `tokenMint` (cached after first fetch),
    ///   fee = `amount` × 10 bps, paid in the same token;
    /// - offline fallback: gateway unreachable → fee 0, payee = sender's own ATA
    ///   (the executor skips the fee transfer when fee == 0).
    ///
    /// The delegate approval must cover `amount + fee`.
    public func createIntentBytes(
        from: String,
        to: String,
        tokenMint: String,
        amount: Int64,
        expiresAt: Int64,
        nonceHex: String? = nil
    ) async throws -> IntentBytesResponse {
        let gatewayWallet = await resolveGatewayWallet()
        let gatewayFeeAccount: String?
        if let gatewayWallet {
            gatewayFeeAccount = try? await deriveAssociatedTokenAccount(ownerWallet: gatewayWallet, tokenMint: tokenMint)
        } else {
            gatewayFeeAccount = nil
        }

        let gasFeePayee: String
        let gasFeeAmount: Int64
        if let gatewayFeeAccount {
            gasFeePayee = gatewayFeeAccount
            gasFeeAmount = amount * Self.gasFeeTakeRateBps / 10_000
        } else {
            // Offline / gateway unreachable: no fee, payee = sender's own token account.
            gasFeePayee = try await deriveAssociatedTokenAccount(ownerWallet: from, tokenMint: tokenMint)
            gasFeeAmount = 0
        }

        let request = CreateIntentBytesRequest(
            from: from,
            to: to,
            tokenMint: tokenMint,
            amount: amount,
            expiresAt: expiresAt,
            gasFeeAmount: gasFeeAmount,
            gasFeePayee: gasFeePayee,
            nonceHex: nonceHex
        )
        return try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_create_intent_bytes($0)) }
            return try FFI.decode(IntentBytesResponse.self, from: raw)
        }
    }

    /// Build the unsigned `revoke` transaction clearing executor delegation.
    public func createRevokeTransaction(
        ownerWallet: String,
        tokenAccounts: [String],
        feePayer: String? = nil,
        recentBlockhash: String,
        tokenProgram: String = "spl-token"
    ) async throws -> String {
        let request = CreateRevokeTransactionRequest(
            ownerWallet: ownerWallet,
            feePayer: feePayer ?? ownerWallet,
            recentBlockhash: recentBlockhash,
            tokenAccounts: tokenAccounts,
            tokenProgram: tokenProgram
        )
        return try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_create_revoke_transaction($0)) }
            return try FFI.decode(RevokeTransactionResponse.self, from: raw).transaction
        }
    }

    /// Latest confirmed blockhash from the configured Solana RPC.
    public func fetchRecentBlockhash() async throws -> String {
        let rpc = try requireRpcUrl()
        let body = #"{"jsonrpc":"2.0","id":1,"method":"getLatestBlockhash","params":[{"commitment":"confirmed"}]}"#
        let data = try await httpPost(rpc, json: body)
        guard
            let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let result = root["result"] as? [String: Any],
            let value = result["value"] as? [String: Any],
            let blockhash = value["blockhash"] as? String
        else {
            throw PolliNetError(
                code: "ERR_RPC",
                message: "Could not parse blockhash from RPC response: \(String(data: data, encoding: .utf8) ?? "<binary>")"
            )
        }
        return blockhash
    }

    /// Submit a fully-signed raw Solana transaction via `sendTransaction`.
    /// Returns the transaction signature.
    public func submitSignedTransaction(signedTxBytes: Data) async throws -> String {
        let rpc = try requireRpcUrl()
        let encoded = signedTxBytes.base64EncodedString()
        let body = #"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[""# + encoded
            + #"",{"encoding":"base64","preflightCommitment":"confirmed"}]}"#
        let data = try await httpPost(rpc, json: body)
        let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        if let signature = root?["result"] as? String {
            return signature
        }
        let message = ((root?["error"] as? [String: Any])?["message"] as? String)
            ?? "sendTransaction failed: \(String(data: data, encoding: .utf8) ?? "<binary>")"
        throw PolliNetError(code: "ERR_RPC", message: message)
    }

    /// All SPL token accounts of `walletAddress` with executor-delegation status.
    /// `isExecutorDelegated == true && delegatedRawAmount > 0` ⇒ Pollinet-ready.
    public func listTokenAccounts(
        walletAddress: String,
        tokenProgramId: String = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    ) async throws -> [DelegatedTokenAccount] {
        let rpc = try requireRpcUrl()

        let executorPda: String
        if let cached = locked({ cachedExecutorPda }) {
            executorPda = cached
        } else {
            executorPda = try await getExecutorPda().pda
            locked { cachedExecutorPda = executorPda }
        }

        let body = #"{"jsonrpc":"2.0","id":1,"method":"getTokenAccountsByOwner","params":[""# + walletAddress
            + #"",{"programId":""# + tokenProgramId
            + #""},{"encoding":"jsonParsed","commitment":"confirmed"}]}"#
        let data = try await httpPost(rpc, json: body)

        guard
            let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let result = root["result"] as? [String: Any]
        else {
            throw PolliNetError(
                code: "ERR_RPC",
                message: "RPC returned no result: \(String(data: data, encoding: .utf8) ?? "<binary>")"
            )
        }
        guard let valueArray = result["value"] as? [[String: Any]] else { return [] }

        return valueArray.compactMap { entry in
            guard
                let pubkey = entry["pubkey"] as? String,
                let account = entry["account"] as? [String: Any],
                let accountData = account["data"] as? [String: Any],
                let parsed = accountData["parsed"] as? [String: Any],
                let info = parsed["info"] as? [String: Any],
                let mint = info["mint"] as? String
            else { return nil }

            let owner = info["owner"] as? String ?? walletAddress
            let tokenAmount = info["tokenAmount"] as? [String: Any]
            let decimals = tokenAmount?["decimals"] as? Int ?? 0
            let balance = (tokenAmount?["amount"] as? String).flatMap(Int64.init) ?? 0

            let delegate = info["delegate"] as? String
            let delegatedAmount = info["delegatedAmount"] as? [String: Any]
            let delegatedRaw = (delegatedAmount?["amount"] as? String).flatMap(Int64.init) ?? 0

            return DelegatedTokenAccount(
                pubkey: pubkey,
                mint: mint,
                owner: owner,
                decimals: decimals,
                rawBalance: balance,
                delegate: delegate,
                delegatedRawAmount: delegatedRaw,
                isExecutorDelegated: delegate == executorPda && delegatedRaw > 0
            )
        }
    }

    /// Submit a signed intent to pollicore for on-chain execution.
    /// Returns the Solana transaction signature.
    public func submitIntent(
        intentBytesBase64: String,
        signatureBase64: String,
        fromTokenAccount: String,
        tokenProgram: String = "spl-token"
    ) async throws -> String {
        let handle = transportHandle
        let request = SubmitIntentFFIRequest(
            intentBytes: intentBytesBase64,
            signature: signatureBase64,
            fromTokenAccount: fromTokenAccount,
            tokenProgram: tokenProgram
        )
        return try await FFI.run {
            let raw = try FFI.withJSON(request) { FFI.consume(pollinet_submit_intent(handle, $0)) }
            return try FFI.decode(SubmitIntentFFIResponse.self, from: raw).txSignature
        }
    }

    /// On-chain intent state for `walletAddress` (no JWT required).
    public func getIntentState(walletAddress: String) async throws -> IntentStateResponse {
        let url = try pollicoreEndpoint("sdk/intents/state", query: [URLQueryItem(name: "wallet", value: walletAddress)])
        return try JSONDecoder().decode(IntentStateResponse.self, from: try await httpGet(url))
    }

    /// Fetch the partially-signed intent-state init transaction from pollicore.
    public func fetchInitTx(walletAddress: String) async throws -> InitTxResponse {
        let url = try pollicoreEndpoint("sdk/intents/init-tx", query: [URLQueryItem(name: "wallet", value: walletAddress)])
        return try JSONDecoder().decode(InitTxResponse.self, from: try await httpGet(url))
    }

    /// Submit the user-signed init transaction; creates the intent-state PDA.
    public func initializeIntentState(
        signedTxBase64: String,
        walletAddress: String
    ) async throws -> InitializeResponse {
        let url = try pollicoreEndpoint("sdk/intents/initialize")
        let payload = try JSONEncoder().encode(InitializeRequest(tx: signedTxBase64, wallet: walletAddress))
        let data = try await httpPost(url.absoluteString, json: String(data: payload, encoding: .utf8) ?? "{}")
        return try JSONDecoder().decode(InitializeResponse.self, from: data)
    }

    @available(*, deprecated, message: "JWT no longer required; use getIntentState(walletAddress:)")
    public func getIntentState(polliCoreBaseUrl: String, authToken: String) async throws -> IntentStateResponse {
        guard let url = URL(string: "\(polliCoreBaseUrl)/sdk/intents/state") else {
            throw PolliNetError(code: "ERR_CONFIG", message: "Invalid pollicore base URL")
        }
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.setValue("Bearer \(authToken)", forHTTPHeaderField: "Authorization")
        let data = try await perform(request)
        return try JSONDecoder().decode(IntentStateResponse.self, from: data)
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Gateway wallet, cached after the first successful fetch. Nil if unreachable.
    private func resolveGatewayWallet() async -> String? {
        if let cached = locked({ cachedGatewayWallet }) {
            return cached
        }
        guard let wallet = try? await getGatewayWallet() else { return nil }
        locked { cachedGatewayWallet = wallet }
        return wallet
    }

    /// Run `body` under the state lock (avoids colliding with Foundation's
    /// NSLock.withLock, which only exists on iOS 16+/macOS 13+).
    private func locked<T>(_ body: () -> T) -> T {
        stateLock.lock()
        defer { stateLock.unlock() }
        return body()
    }

    /// Endpoint under the pollicore base URL baked into the Rust core at compile time.
    /// iOS 15-compatible URL composition (no URL.appending(path:)).
    private func pollicoreEndpoint(_ path: String, query: [URLQueryItem]? = nil) throws -> URL {
        let raw = FFI.consume(pollinet_get_pollicore_url())
        guard !raw.isEmpty else {
            throw PolliNetError(
                code: "ERR_CONFIG",
                message: "POLLICORE_URL not configured — set it in .env before building the Rust core"
            )
        }
        let base = raw.hasSuffix("/") ? String(raw.dropLast()) : raw
        guard var components = URLComponents(string: "\(base)/\(path)") else {
            throw PolliNetError(code: "ERR_CONFIG", message: "Invalid pollicore URL: \(raw)")
        }
        components.queryItems = query
        guard let url = components.url else {
            throw PolliNetError(code: "ERR_CONFIG", message: "Invalid pollicore URL: \(raw)")
        }
        return url
    }

    private func requireRpcUrl() throws -> String {
        guard let rpcUrl else {
            throw PolliNetError(code: "ERR_CONFIG", message: "rpcUrl not set on SdkConfig")
        }
        return rpcUrl
    }

    private func httpGet(_ url: URL) async throws -> Data {
        try await perform(URLRequest(url: url, timeoutInterval: 30))
    }

    private func httpPost(_ urlString: String, json body: String) async throws -> Data {
        guard let url = URL(string: urlString) else {
            throw PolliNetError(code: "ERR_CONFIG", message: "Invalid URL: \(urlString)")
        }
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body.data(using: .utf8)
        return try await perform(request)
    }

    private func perform(_ request: URLRequest) async throws -> Data {
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw PolliNetError(code: "ERR_HTTP", message: "Non-HTTP response")
        }
        guard (200...299).contains(http.statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw PolliNetError(
                code: "HTTP_\(http.statusCode)",
                message: "pollicore error \(http.statusCode): \(body)"
            )
        }
        return data
    }
}
