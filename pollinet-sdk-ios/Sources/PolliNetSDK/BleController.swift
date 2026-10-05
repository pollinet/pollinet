//
//  BleController.swift
//  CoreBluetooth driver for the PolliNet mesh — the iOS counterpart of
//  Android's BleService.kt, speaking the exact same GATT + frame contract:
//
//    service 00001820-…  TX 00001821-… (notify)  RX 00001822-… (write)
//    frames: first byte 0x08 CONFIRMATION (pollicore-signed) · 0x09 TX_ABORT ·
//    0x0A DRAIN_READY · 0x0B CLOSE_ACK · 0x0C CONFIRMATION_FRAG (chunked JSON,
//    header [type, totalChunks u8, chunkIndex u8, totalLen u16 LE]) ·
//    0x7B '{' legacy single-packet JSON confirmation · anything else = bincode
//    TransactionFragment → pushInbound.
//
//  The host drives the radio; ALL protocol state (reassembly, dedup, queues,
//  retry, tombstones, rotation policy inputs) lives in the Rust engine behind
//  the PolliNetSDK byte pump. Keep logic out of this file that isn't radio
//  plumbing.
//
//  iOS-vs-Android caveats (documented in plan-ios.md):
//  - Backgrounded iOS advertises the service UUID in the "overflow area",
//    which Android scanners cannot see. iOS→Android discovery therefore only
//    works while this app is foregrounded; iOS-central ↔ Android-peripheral
//    keeps working in background (bluetooth-central background mode).
//  - iOS exposes no peer MAC address; the compact peer id is derived from
//    CoreBluetooth's per-device UUID instead (first 4 bytes of SHA-256, same
//    derivation shape as Android's MAC-based id — local bookkeeping only).
//

import Combine
@preconcurrency import CoreBluetooth
import CryptoKit
import Foundation
import Network

// MARK: - Public observable types

public enum BleConnectionState: String, Sendable {
    case disconnected, scanning, connecting, connected, error
}

public struct DiscoveredPeer: Identifiable, Sendable {
    public let id: String          // CoreBluetooth peripheral UUID string
    public var rssi: Int
    public var lastSeenAt: Date
    public var isConnected: Bool
}

public enum BleConfirmationEvent: Sendable {
    case success(txIdShort: String, detail: String)
    case failure(txIdShort: String, detail: String)
}

public struct ConfirmationRecord: Identifiable, Sendable {
    public let id = UUID()
    public let txIdShort: String
    public let success: Bool
    public let detail: String
    public let at: Date
}

public struct ReceivedTxRecord: Identifiable, Sendable {
    public enum Status: String, Sendable { case received, submitted, relayed, failed }
    public let id: String          // txId
    public var status: Status
    public var detail: String
    public var at: Date
}

// MARK: - Controller

public final class BleController: NSObject, ObservableObject, @unchecked Sendable {

    // GATT contract — must match BleService.kt exactly.
    public static let serviceUUID = CBUUID(string: "00001820-0000-1000-8000-00805f9b34fb")
    public static let txCharUUID = CBUUID(string: "00001821-0000-1000-8000-00805f9b34fb")
    public static let rxCharUUID = CBUUID(string: "00001822-0000-1000-8000-00805f9b34fb")

    // Wire/timing constants mirrored from BleService.kt.
    private static let confFragHeaderSize = 5
    private static let confFragTimeout: TimeInterval = 5.0
    private static let idleDisconnectWindow: TimeInterval = 4.0
    private static let sendLoopInterval: TimeInterval = 0.8
    private static let workerFallbackInterval: TimeInterval = 30.0
    private static let maxTxRelayHops = 5
    private static let maxLogLines = 300

    private let sdk: PolliNetSDK
    private let queue = DispatchQueue(label: "xyz.pollinet.ble")

    // Radios
    private var central: CBCentralManager?
    private var peripheralManager: CBPeripheralManager?

    // Central-role state
    private var connectedPeripheral: CBPeripheral?
    private var remoteRxCharacteristic: CBCharacteristic?
    private var remoteTxCharacteristic: CBCharacteristic?

    // Peripheral-role state
    private var txCharacteristic: CBMutableCharacteristic?
    private var subscribedCentral: CBCentral?
    private var pendingNotifyData: Data?   // retry buffer for updateValue backpressure

    // Session state (mirrors BleService's sending loop bookkeeping)
    private var activeTxId: String?
    private var queueEmptySince: Date?
    private var lastInboundAt = Date.distantPast
    private var operationInProgress = false

    // Confirmation-fragment (0x0C) reassembly state
    private var confFragBuffer: [Data] = []
    private var confFragTotalChunks = 0
    private var confFragTotalBytes = 0
    private var confFragLastUpdate = Date.distantPast
    private var seenConfirmationTxIds = Set<String>()

    // Loops
    private var sendLoopTask: Task<Void, Never>?
    private var workerTask: Task<Void, Never>?
    private let workSignal = AsyncStream<Void>.makeStream()

    // Internet gating for the relay/RPC path (Android: ConnectivityManager callback)
    private let pathMonitor = NWPathMonitor()
    private var hasInternet = false

    // MARK: Observable state (mirrors BleService's StateFlows)

    @Published public private(set) var connectionState: BleConnectionState = .disconnected
    @Published public private(set) var isScanning = false
    @Published public private(set) var isAdvertising = false
    @Published public private(set) var peers: [String: DiscoveredPeer] = [:]
    @Published public private(set) var latestMetrics: MetricsSnapshot?
    @Published public private(set) var logs: [String] = []
    @Published public private(set) var receivedTransactions: [ReceivedTxRecord] = []
    @Published public private(set) var confirmationLog: [ConfirmationRecord] = []

    private var confirmationContinuations: [UUID: AsyncStream<BleConfirmationEvent>.Continuation] = [:]

    /// Stream of confirmation success/failure events (SharedFlow counterpart).
    public var confirmationEvents: AsyncStream<BleConfirmationEvent> {
        AsyncStream { continuation in
            let id = UUID()
            queue.async { self.confirmationContinuations[id] = continuation }
            continuation.onTermination = { [weak self] _ in
                self?.queue.async { self?.confirmationContinuations.removeValue(forKey: id) }
            }
        }
    }

    public init(sdk: PolliNetSDK) {
        self.sdk = sdk
        super.init()
        pathMonitor.pathUpdateHandler = { [weak self] path in
            self?.hasInternet = path.status == .satisfied
        }
        pathMonitor.start(queue: queue)
    }

    // MARK: - Lifecycle

    /// Bring up both radios and the worker loops.
    public func start() {
        queue.async {
            if self.central == nil {
                var options: [String: Any] = [:]
                #if os(iOS)
                options[CBCentralManagerOptionRestoreIdentifierKey] = "xyz.pollinet.central"
                #endif
                self.central = CBCentralManager(delegate: self, queue: self.queue, options: options)
            }
            if self.peripheralManager == nil {
                var options: [String: Any] = [:]
                #if os(iOS)
                options[CBPeripheralManagerOptionRestoreIdentifierKey] = "xyz.pollinet.peripheral"
                #endif
                self.peripheralManager = CBPeripheralManager(delegate: self, queue: self.queue, options: options)
            }
        }
        startSendLoop()
        startWorker()
        log("🚀 BleController started")
    }

    /// Stop radios and loops (counterpart of service onDestroy).
    public func stop() {
        sendLoopTask?.cancel()
        workerTask?.cancel()
        queue.async {
            self.stopScanningLocked()
            self.stopAdvertisingLocked()
            if let peripheral = self.connectedPeripheral {
                self.central?.cancelPeripheralConnection(peripheral)
            }
            self.peripheralManager?.removeAllServices()
            self.setConnectionState(.disconnected)
        }
        log("🛑 BleController stopped")
    }

    // MARK: - Scanning / advertising controls

    public func startScanning() {
        queue.async {
            guard let central = self.central, central.state == .poweredOn else {
                self.log("⚠️ Central not powered on — scan deferred")
                return
            }
            guard !central.isScanning else { return }
            central.scanForPeripherals(
                withServices: [Self.serviceUUID],
                options: [CBCentralManagerScanOptionAllowDuplicatesKey: true]
            )
            DispatchQueue.main.async { self.isScanning = true }
            if self.connectionState == .disconnected {
                self.setConnectionState(.scanning)
            }
            self.log("🔍 Scanning for PolliNet peers…")
        }
    }

    public func stopScanning() {
        queue.async { self.stopScanningLocked() }
    }

    private func stopScanningLocked() {
        central?.stopScan()
        DispatchQueue.main.async { self.isScanning = false }
        if connectionState == .scanning { setConnectionState(.disconnected) }
    }

    public func startAdvertising() {
        queue.async {
            guard let pm = self.peripheralManager, pm.state == .poweredOn else {
                self.log("⚠️ Peripheral not powered on — advertise deferred")
                return
            }
            guard !pm.isAdvertising else { return }
            self.setupGattServiceIfNeeded()
            pm.startAdvertising([
                CBAdvertisementDataServiceUUIDsKey: [Self.serviceUUID],
                CBAdvertisementDataLocalNameKey: "PolliNet",
            ])
            DispatchQueue.main.async { self.isAdvertising = true }
            self.log("📢 Advertising PolliNet service")
        }
    }

    public func stopAdvertising() {
        queue.async { self.stopAdvertisingLocked() }
    }

    private func stopAdvertisingLocked() {
        peripheralManager?.stopAdvertising()
        DispatchQueue.main.async { self.isAdvertising = false }
    }

    // MARK: - Transaction entry point

    /// Verify, fragment, and queue a signed payload for mesh propagation
    /// (base64 raw Solana tx OR intent-envelope JSON), then kick the pump.
    public func queueTransaction(base64: String, maxPayload: Int? = nil) async throws -> String {
        let txId = try await sdk.acceptAndQueueExternalTransaction(base64SignedTx: base64, maxPayload: maxPayload)
        log("📬 Queued tx \(txId.prefix(8))… for propagation")
        signalWork()
        return txId
    }

    public func clearLogs() {
        DispatchQueue.main.async { self.logs = [] }
    }

    // MARK: - Send loop (counterpart of ensureSendingLoopStarted + sendNextOutbound)

    private func startSendLoop() {
        guard sendLoopTask == nil else { return }
        sendLoopTask = Task.detached { [weak self] in
            while !Task.isCancelled {
                await self?.sendNextOutbound()
                try? await Task.sleep(nanoseconds: UInt64(Self.sendLoopInterval * 1_000_000_000))
            }
        }
    }

    private func sendNextOutbound() async {
        guard connectionState == .connected, !operationInProgress else { return }

        let cap = effectivePayloadCap()
        var data = await sdk.nextOutbound(maxLen: cap)

        // Mid-session refill: the transport's low-level queue only refills via
        // loadForSending(). Only when no tx is active, else in-flight fragments
        // would be re-sent forever (relevance decrements on idle-disconnect).
        if data == nil, activeTxId == nil {
            if let loaded = try? await sdk.loadForSending() {
                activeTxId = loaded.txId
                log("📡 Loaded tx \(loaded.txId.prefix(8))… mid-session (relevance=\(loaded.relevance), fragments=\(loaded.fragmentCount))")
                data = await sdk.nextOutbound(maxLen: cap)
            }
        }

        guard let frame = data else {
            await handleIdleWindow()
            return
        }

        queueEmptySince = nil
        sendToGatt(frame)
    }

    /// Both queues empty: give the peer a 4 s window to push data, then
    /// disconnect to rotate the mesh (mutual drain → confirmDeliveredByPeer).
    private func handleIdleWindow() async {
        if queueEmptySince == nil {
            queueEmptySince = Date()
            log("📭 Queue empty — opening \(Int(Self.idleDisconnectWindow))s idle window")
            return
        }
        let idleStart = max(queueEmptySince ?? Date(), lastInboundAt)
        guard Date().timeIntervalSince(idleStart) >= Self.idleDisconnectWindow else { return }

        guard connectionState == .connected else {
            queueEmptySince = nil
            return
        }

        if let txId = activeTxId {
            let peerId = compactPeerId(currentPeerIdentifier())
            let removed = (try? await sdk.confirmDeliveredByPeer(txId: txId, peerIdHex: peerId)) ?? true
            log(removed
                ? "TX \(txId.prefix(8))… fan-out exhausted — evicted"
                : "TX \(txId.prefix(8))… delivered to \(peerId) — relevance decremented")
            activeTxId = nil
        }
        queueEmptySince = nil

        log("🔄 Dancing mesh: idle window expired — disconnecting to rotate")
        queue.async {
            if let peripheral = self.connectedPeripheral {
                let peerId = peripheral.identifier.uuidString
                Task { [weak self] in
                    if let params = try? await self?.sdk.getAdaptiveParams() {
                        try? await self?.sdk.addPeerToCooldown(peerId: peerId, cooldownMs: params.cooldownMs)
                    }
                }
                self.central?.cancelPeripheralConnection(peripheral)
            } else {
                // Peripheral role: we cannot force-disconnect a central on iOS;
                // stop notifying and let the peer's idle window close the link.
                self.setConnectionState(self.isAdvertising ? .disconnected : self.connectionState)
            }
        }
    }

    /// Usable payload per ATT operation for the current link. CoreBluetooth
    /// exposes the post-MTU-negotiation caps directly (Android: MTU − 10).
    private func effectivePayloadCap() -> Int {
        var cap = 244 // sensible default ≈ MTU 247 − 3
        if let peripheral = connectedPeripheral {
            cap = peripheral.maximumWriteValueLength(for: .withResponse)
        } else if let central = subscribedCentral {
            cap = central.maximumUpdateValueLength
        }
        return max(20, min(cap, 512))
    }

    private func sendToGatt(_ data: Data) {
        queue.async {
            // Client path: write to the remote RX characteristic (with response,
            // mirroring Android's WRITE_TYPE_DEFAULT — didWrite paces the pump).
            if let peripheral = self.connectedPeripheral, let rx = self.remoteRxCharacteristic {
                self.operationInProgress = true
                peripheral.writeValue(data, for: rx, type: .withResponse)
                return
            }
            // Server path: notify the subscribed central on TX.
            if let pm = self.peripheralManager, let tx = self.txCharacteristic, self.subscribedCentral != nil {
                let ok = pm.updateValue(data, for: tx, onSubscribedCentrals: nil)
                if !ok {
                    // Backpressure: retry this frame in peripheralManagerIsReady.
                    self.pendingNotifyData = data
                }
                return
            }
            self.log("❌ No active link to send \(data.count)B frame")
        }
    }

    // MARK: - Inbound dispatch (counterpart of handleReceivedData)

    private func handleReceivedData(_ data: Data) {
        lastInboundAt = Date()
        guard let first = data.first else { return }

        switch first {
        case 0x08, 0x09:
            // Pollicore-signed confirmation / TX_ABORT — Rust owns both.
            Task { [weak self] in
                guard let self else { return }
                if let result = try? await self.sdk.ingestConfirmation(confirmationBytes: data) {
                    self.log("Confirmation ingested: purged=\(result.purged) carrier=\(result.addedToCarrier)")
                    if result.purged {
                        self.emitConfirmation(.success(txIdShort: "", detail: "Confirmed on Solana"))
                        self.recordConfirmation(txIdShort: "", success: true, detail: "Confirmed on Solana")
                    }
                }
            }
        case 0x0A:
            log("Received DRAIN_READY — peer queue drained")
        case 0x0B:
            log("Received CLOSE_ACK")
        case 0x0C:
            handleConfirmationFragment(data)
        case 0x7B: // '{' legacy single-packet JSON confirmation
            handleReceivedConfirmation(data)
        default:
            // Data fragment → Rust reassembly, then check for completed txs.
            Task { [weak self] in
                guard let self else { return }
                do {
                    try await self.sdk.pushInbound(data)
                    if let metrics = try? await self.sdk.metrics() {
                        DispatchQueue.main.async { self.latestMetrics = metrics }
                    }
                    let queued = (try? await self.sdk.getReceivedQueueSize()) ?? 0
                    if queued > 0 {
                        self.log("🎉 Transaction reassembly complete (queue=\(queued))")
                        self.signalWork()
                    }
                } catch {
                    self.log("❌ pushInbound failed: \(error.localizedDescription)")
                }
            }
        }
    }

    // MARK: - Confirmations (JSON path, wire-identical to Android)

    private func handleReceivedConfirmation(_ data: Data) {
        guard let confirmation = try? JSONDecoder().decode(Confirmation.self, from: data) else {
            log("❌ Failed to decode confirmation JSON (\(data.count)B)")
            return
        }
        // Echo-loop guard: first receive wins.
        guard seenConfirmationTxIds.insert(confirmation.txId).inserted else { return }

        let txShort = String(confirmation.txId.prefix(8))
        switch confirmation.status {
        case .success(let signature):
            emitConfirmation(.success(txIdShort: txShort, detail: String(signature.prefix(16))))
            recordConfirmation(txIdShort: txShort, success: true, detail: String(signature.prefix(16)))
        case .failed(let error):
            emitConfirmation(.failure(txIdShort: txShort, detail: error))
            recordConfirmation(txIdShort: txShort, success: false, detail: error)
        }

        if confirmation.relayCount < Self.maxTxRelayHops {
            Task { [weak self] in
                try? await self?.sdk.relayConfirmation(confirmation)
                self?.signalWork()
            }
        }
    }

    private func handleConfirmationFragment(_ data: Data) {
        guard data.count > Self.confFragHeaderSize else { return }
        let bytes = [UInt8](data)
        let totalChunks = Int(bytes[1])
        let chunkIndex = Int(bytes[2])
        let totalBytes = Int(bytes[3]) | (Int(bytes[4]) << 8)
        let chunk = data.dropFirst(Self.confFragHeaderSize)

        let stale = confFragLastUpdate != .distantPast
            && Date().timeIntervalSince(confFragLastUpdate) > Self.confFragTimeout
        let newSequence = chunkIndex == 0
            || confFragTotalChunks != totalChunks
            || confFragTotalBytes != totalBytes
        if stale || newSequence {
            confFragBuffer.removeAll()
            confFragTotalChunks = totalChunks
            confFragTotalBytes = totalBytes
        }
        confFragBuffer.append(Data(chunk))
        confFragLastUpdate = Date()

        guard confFragBuffer.count >= confFragTotalChunks else { return }

        var assembled = Data(capacity: confFragTotalBytes)
        for fragment in confFragBuffer { assembled.append(fragment) }
        assembled = assembled.prefix(confFragTotalBytes)
        let expected = confFragTotalBytes
        confFragBuffer.removeAll()
        confFragTotalChunks = 0
        confFragTotalBytes = 0
        confFragLastUpdate = .distantPast

        guard assembled.count == expected else {
            log("⚠️ Confirmation reassembly size mismatch — dropping")
            return
        }
        handleReceivedConfirmation(assembled)
    }

    /// Pop up to 10 queued confirmations and relay them over the link —
    /// single packet when ≤ cap, else 0x0C chunked (50 ms spacing).
    private func processConfirmationQueue() async {
        guard connectionState == .connected else { return }
        for _ in 0..<10 {
            guard let confirmation = try? await sdk.popConfirmation() else { break }
            guard let jsonBytes = try? JSONEncoder().encode(confirmation) else { continue }

            let singlePacketCap = max(20, effectivePayloadCap())
            if jsonBytes.count <= singlePacketCap {
                sendToGatt(jsonBytes)
            } else {
                await sendFragmentedConfirmation(jsonBytes, chunkCap: singlePacketCap)
            }
        }
    }

    private func sendFragmentedConfirmation(_ jsonBytes: Data, chunkCap: Int) async {
        let maxChunkPayload = max(20, chunkCap - Self.confFragHeaderSize)
        let totalChunks = (jsonBytes.count + maxChunkPayload - 1) / maxChunkPayload
        guard totalChunks <= 255, jsonBytes.count <= 0xFFFF else {
            log("❌ Confirmation \(jsonBytes.count)B too large to fragment")
            return
        }
        for i in 0..<totalChunks {
            let start = i * maxChunkPayload
            let end = min(start + maxChunkPayload, jsonBytes.count)
            var frame = Data(capacity: Self.confFragHeaderSize + (end - start))
            frame.append(0x0C)
            frame.append(UInt8(totalChunks))
            frame.append(UInt8(i))
            frame.append(UInt8(jsonBytes.count & 0xFF))
            frame.append(UInt8((jsonBytes.count >> 8) & 0xFF))
            frame.append(jsonBytes[start..<end])
            sendToGatt(frame)
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    // MARK: - Unified worker (counterpart of startUnifiedEventWorker)

    private func signalWork() {
        workSignal.continuation.yield()
    }

    private func startWorker() {
        guard workerTask == nil else { return }
        workerTask = Task.detached { [weak self] in
            guard let self else { return }
            let timer = AsyncStream<Void> { continuation in
                let task = Task {
                    while !Task.isCancelled {
                        try? await Task.sleep(nanoseconds: UInt64(Self.workerFallbackInterval * 1_000_000_000))
                        continuation.yield()
                    }
                }
                continuation.onTermination = { _ in task.cancel() }
            }
            await withTaskGroup(of: Void.self) { group in
                group.addTask { for await _ in self.workSignal.stream { await self.runWorkPass() } }
                group.addTask { for await _ in timer { await self.runWorkPass() } }
            }
        }
    }

    private func runWorkPass() async {
        // Rust-side periodic work: retry/timeout frames to (re)send.
        if let frames = try? await sdk.tick() {
            for base64 in frames {
                if let data = Data(base64Encoded: base64), connectionState == .connected {
                    sendToGatt(data)
                }
            }
        }
        await processReceivedQueue()
        await processRetryQueue()
        await processConfirmationQueue()
        try? await sdk.periodicMaintenance()
        try? await sdk.autoSaveQueues()
        if let metrics = try? await sdk.metrics() {
            DispatchQueue.main.async { self.latestMetrics = metrics }
        }
    }

    // MARK: - Relay submission (counterpart of processReceivedQueue + submitReceivedPayload)

    private struct IntentEnvelope: Codable {
        let intentBytes: String
        let signature: String
        let fromTokenAccount: String
        var tokenProgram: String = "spl-token"

        enum CodingKeys: String, CodingKey {
            case intentBytes = "intent_bytes"
            case signature
            case fromTokenAccount = "from_token_account"
            case tokenProgram = "token_program"
        }
    }

    private func processReceivedQueue() async {
        guard hasInternet else { return }
        while let received = try? await sdk.nextReceivedTransaction() {
            let txShort = String(received.txId.prefix(8))
            upsertReceivedTx(id: received.txId, status: .received, detail: "reassembled")
            do {
                let signature = try await submitReceivedPayload(base64: received.transactionBase64)
                if let raw = Data(base64Encoded: received.transactionBase64) {
                    _ = try? await sdk.markTransactionSubmitted(transactionBytes: raw)
                }
                try? await sdk.queueConfirmation(txId: received.txId, signature: signature)
                upsertReceivedTx(id: received.txId, status: .submitted, detail: String(signature.prefix(16)))
                log("✅ Relayed tx \(txShort)… → \(signature.prefix(16))…")
                signalWork() // confirmation is now queued — push it out
            } catch {
                let message = error.localizedDescription
                upsertReceivedTx(id: received.txId, status: .failed, detail: message)
                if let raw = Data(base64Encoded: received.transactionBase64) {
                    try? await sdk.addToRetryQueue(txBytes: raw, txId: received.txId, error: message)
                }
                try? await sdk.queueFailureConfirmation(txId: received.txId, error: message)
                log("❌ Relay failed for \(txShort)…: \(message)")
            }
        }
    }

    private func processRetryQueue() async {
        guard hasInternet else { return }
        while let retry = try? await sdk.popReadyRetry() {
            do {
                let signature = try await submitReceivedPayload(base64: retry.txBytes)
                try? await sdk.queueConfirmation(txId: retry.txId, signature: signature)
                log("✅ Retry succeeded for \(retry.txId.prefix(8))…")
            } catch {
                try? await sdk.addToRetryQueue(
                    txBytes: Data(base64Encoded: retry.txBytes) ?? Data(),
                    txId: retry.txId,
                    error: error.localizedDescription
                )
            }
        }
    }

    /// Route an intent-envelope JSON to pollicore, a raw signed tx to Solana RPC.
    private func submitReceivedPayload(base64: String) async throws -> String {
        guard let raw = Data(base64Encoded: base64) else {
            throw PolliNetError(code: "ERR_DECODE", message: "Received payload is not valid base64")
        }
        if raw.first == 0x7B, // '{'
           let envelope = try? JSONDecoder().decode(IntentEnvelope.self, from: raw) {
            return try await sdk.submitIntent(
                intentBytesBase64: envelope.intentBytes,
                signatureBase64: envelope.signature,
                fromTokenAccount: envelope.fromTokenAccount,
                tokenProgram: envelope.tokenProgram
            )
        }
        return try await sdk.submitSignedTransaction(signedTxBytes: raw)
    }

    // MARK: - Peer bookkeeping

    /// Compact 4-byte peer id: first 4 bytes of SHA-256 of the peer identifier
    /// (Android hashes the MAC; iOS hashes CoreBluetooth's per-device UUID).
    private func compactPeerId(_ identifier: String) -> String {
        let digest = SHA256.hash(data: Data(identifier.utf8))
        return digest.prefix(4).map { String(format: "%02x", $0) }.joined()
    }

    private func currentPeerIdentifier() -> String {
        connectedPeripheral?.identifier.uuidString
            ?? subscribedCentral?.identifier.uuidString
            ?? ""
    }

    private func recordPeer(id: String, rssi: Int, connected: Bool) {
        DispatchQueue.main.async {
            var peer = self.peers[id] ?? DiscoveredPeer(id: id, rssi: rssi, lastSeenAt: Date(), isConnected: connected)
            peer.rssi = rssi
            peer.lastSeenAt = Date()
            peer.isConnected = connected
            self.peers[id] = peer
        }
    }

    private func upsertReceivedTx(id: String, status: ReceivedTxRecord.Status, detail: String) {
        DispatchQueue.main.async {
            if let index = self.receivedTransactions.firstIndex(where: { $0.id == id }) {
                self.receivedTransactions[index].status = status
                self.receivedTransactions[index].detail = detail
                self.receivedTransactions[index].at = Date()
            } else {
                self.receivedTransactions.insert(
                    ReceivedTxRecord(id: id, status: status, detail: detail, at: Date()), at: 0
                )
                if self.receivedTransactions.count > 50 { self.receivedTransactions.removeLast() }
            }
        }
    }

    private func recordConfirmation(txIdShort: String, success: Bool, detail: String) {
        DispatchQueue.main.async {
            self.confirmationLog.insert(
                ConfirmationRecord(txIdShort: txIdShort, success: success, detail: detail, at: Date()), at: 0
            )
            if self.confirmationLog.count > 50 { self.confirmationLog.removeLast() }
        }
    }

    private func emitConfirmation(_ event: BleConfirmationEvent) {
        queue.async {
            for continuation in self.confirmationContinuations.values {
                continuation.yield(event)
            }
        }
    }

    private func setConnectionState(_ state: BleConnectionState) {
        DispatchQueue.main.async { self.connectionState = state }
    }

    private func log(_ line: String) {
        print("[PolliNet.BLE] \(line)") // Xcode console (logcat counterpart)
        DispatchQueue.main.async {
            self.logs.append(line)
            if self.logs.count > Self.maxLogLines {
                self.logs.removeFirst(self.logs.count - Self.maxLogLines)
            }
        }
    }

    /// Human-readable radio state + authorization, for instant permission triage.
    private func describe(_ state: CBManagerState) -> String {
        let name: String
        switch state {
        case .unknown: name = "unknown"
        case .resetting: name = "resetting"
        case .unsupported: name = "unsupported"
        case .unauthorized: name = "UNAUTHORIZED — grant Bluetooth in Settings → PolliNetExample"
        case .poweredOff: name = "poweredOff — turn Bluetooth on"
        case .poweredOn: name = "poweredOn"
        @unknown default: name = "state(\(state.rawValue))"
        }
        let auth: String
        switch CBManager.authorization {
        case .notDetermined: auth = "notDetermined (permission prompt pending)"
        case .restricted: auth = "restricted (Screen Time / MDM)"
        case .denied: auth = "denied"
        case .allowedAlways: auth = "allowed"
        @unknown default: auth = "authorization(\(CBManager.authorization.rawValue))"
        }
        return "\(name) · auth: \(auth)"
    }

    // MARK: - GATT service (peripheral role)

    private var serviceInstalled = false

    private func setupGattServiceIfNeeded() {
        guard !serviceInstalled, let pm = peripheralManager else { return }
        let tx = CBMutableCharacteristic(
            type: Self.txCharUUID,
            properties: [.notify],
            value: nil,
            permissions: []
        )
        let rx = CBMutableCharacteristic(
            type: Self.rxCharUUID,
            properties: [.write, .writeWithoutResponse],
            value: nil,
            permissions: [.writeable]
        )
        let service = CBMutableService(type: Self.serviceUUID, primary: true)
        service.characteristics = [tx, rx]
        pm.add(service)
        txCharacteristic = tx
        serviceInstalled = true
    }

    /// Session start housekeeping shared by both roles.
    private func onLinkEstablished(peerIdentifier: String) {
        setConnectionState(.connected)
        queueEmptySince = nil
        activeTxId = nil
        recordPeer(id: peerIdentifier, rssi: 0, connected: true)
        Task { [weak self] in
            guard let self else { return }
            // Drop stale relayed data, then load the best tx for this session.
            _ = try? await self.sdk.purgeStaleOutbound(maxAgeSecs: 300)
            if let loaded = try? await self.sdk.loadForSending() {
                self.activeTxId = loaded.txId
                self.log("📡 Loaded tx \(loaded.txId.prefix(8))… at connect (fragments=\(loaded.fragmentCount))")
            }
            try? await self.sdk.recordPeerHeartbeat(peerId: peerIdentifier)
        }
        log("🔗 Link established with \(peerIdentifier.prefix(8))…")
    }

    private func onLinkClosed(peerIdentifier: String) {
        recordPeer(id: peerIdentifier, rssi: 0, connected: false)
        operationInProgress = false
        queueEmptySince = nil
        setConnectionState(isScanning ? .scanning : .disconnected)
        log("🔌 Link closed with \(peerIdentifier.prefix(8))…")
    }
}

// MARK: - CBCentralManagerDelegate (client role — mirrors gattCallback)

extension BleController: CBCentralManagerDelegate {

    public func centralManagerDidUpdateState(_ central: CBCentralManager) {
        log("🔵 Central state: \(describe(central.state))")
        if central.state == .poweredOn, isScanning {
            startScanning()
        }
    }

    #if os(iOS)
    public func centralManager(_ central: CBCentralManager, willRestoreState dict: [String: Any]) {
        if let peripherals = dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral],
           let peripheral = peripherals.first {
            connectedPeripheral = peripheral
            peripheral.delegate = self
            log("♻️ Restored central session with \(peripheral.identifier.uuidString.prefix(8))…")
        }
    }
    #endif

    public func centralManager(
        _ central: CBCentralManager,
        didDiscover peripheral: CBPeripheral,
        advertisementData: [String: Any],
        rssi RSSI: NSNumber
    ) {
        let peerId = peripheral.identifier.uuidString
        recordPeer(id: peerId, rssi: RSSI.intValue, connected: false)

        Task { [weak self] in
            guard let self else { return }
            // Density estimation input (Subsystem 1) + RSSI for health.
            try? await self.sdk.recordScanResult(peerId: peerId)
            try? await self.sdk.recordPeerRssi(peerId: peerId, rssi: RSSI.intValue)

            // Respect cooldowns; connect to the first eligible peer while idle.
            guard self.connectionState == .scanning || self.connectionState == .disconnected else { return }
            let cooling = (try? await self.sdk.isPeerInCooldown(peerId: peerId)) ?? false
            guard !cooling else { return }

            self.queue.async {
                guard self.connectedPeripheral == nil else { return }
                self.connectedPeripheral = peripheral
                peripheral.delegate = self
                self.setConnectionState(.connecting)
                self.log("🤝 Connecting to \(peerId.prefix(8))… (RSSI \(RSSI.intValue))")
                central.connect(peripheral, options: nil)
            }
        }
    }

    public func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        peripheral.discoverServices([Self.serviceUUID])
    }

    public func centralManager(
        _ central: CBCentralManager,
        didFailToConnect peripheral: CBPeripheral,
        error: Error?
    ) {
        log("❌ Connect failed: \(error?.localizedDescription ?? "unknown")")
        if connectedPeripheral?.identifier == peripheral.identifier {
            connectedPeripheral = nil
        }
        setConnectionState(isScanning ? .scanning : .disconnected)
    }

    public func centralManager(
        _ central: CBCentralManager,
        didDisconnectPeripheral peripheral: CBPeripheral,
        error: Error?
    ) {
        let peerId = peripheral.identifier.uuidString
        if connectedPeripheral?.identifier == peripheral.identifier {
            connectedPeripheral = nil
            remoteRxCharacteristic = nil
            remoteTxCharacteristic = nil
        }
        onLinkClosed(peerIdentifier: peerId)
    }
}

// MARK: - CBPeripheralDelegate (client role)

extension BleController: CBPeripheralDelegate {

    public func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard error == nil,
              let service = peripheral.services?.first(where: { $0.uuid == Self.serviceUUID }) else {
            log("❌ PolliNet service not found on peer")
            central?.cancelPeripheralConnection(peripheral)
            return
        }
        peripheral.discoverCharacteristics([Self.txCharUUID, Self.rxCharUUID], for: service)
    }

    public func peripheral(
        _ peripheral: CBPeripheral,
        didDiscoverCharacteristicsFor service: CBService,
        error: Error?
    ) {
        guard error == nil, let characteristics = service.characteristics else { return }
        for characteristic in characteristics {
            switch characteristic.uuid {
            case Self.txCharUUID:
                remoteTxCharacteristic = characteristic
                peripheral.setNotifyValue(true, for: characteristic) // CCCD subscribe
            case Self.rxCharUUID:
                remoteRxCharacteristic = characteristic
            default:
                break
            }
        }
        if remoteRxCharacteristic != nil, remoteTxCharacteristic != nil {
            onLinkEstablished(peerIdentifier: peripheral.identifier.uuidString)
        }
    }

    public func peripheral(
        _ peripheral: CBPeripheral,
        didUpdateValueFor characteristic: CBCharacteristic,
        error: Error?
    ) {
        guard error == nil, characteristic.uuid == Self.txCharUUID,
              let data = characteristic.value else { return }
        handleReceivedData(data)
    }

    public func peripheral(
        _ peripheral: CBPeripheral,
        didWriteValueFor characteristic: CBCharacteristic,
        error: Error?
    ) {
        operationInProgress = false
        if let error {
            log("❌ Write failed: \(error.localizedDescription)")
        }
    }

    public func peripheral(_ peripheral: CBPeripheral, didReadRSSI RSSI: NSNumber, error: Error?) {
        guard error == nil else { return }
        let peerId = peripheral.identifier.uuidString
        recordPeer(id: peerId, rssi: RSSI.intValue, connected: true)
        Task { [weak self] in
            try? await self?.sdk.recordPeerRssi(peerId: peerId, rssi: RSSI.intValue)
        }
    }
}

// MARK: - CBPeripheralManagerDelegate (server role — mirrors gattServerCallback)

extension BleController: CBPeripheralManagerDelegate {

    public func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        log("🟣 Peripheral state: \(describe(peripheral.state))")
        if peripheral.state == .poweredOn, isAdvertising {
            startAdvertising()
        }
    }

    #if os(iOS)
    public func peripheralManager(_ peripheral: CBPeripheralManager, willRestoreState dict: [String: Any]) {
        log("♻️ Peripheral session restored")
        serviceInstalled = (dict[CBPeripheralManagerRestoredStateServicesKey] as? [CBMutableService])?
            .contains { $0.uuid == Self.serviceUUID } ?? false
    }
    #endif

    public func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didSubscribeTo characteristic: CBCharacteristic
    ) {
        guard characteristic.uuid == Self.txCharUUID else { return }
        subscribedCentral = central
        onLinkEstablished(peerIdentifier: central.identifier.uuidString)
    }

    public func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didUnsubscribeFrom characteristic: CBCharacteristic
    ) {
        guard characteristic.uuid == Self.txCharUUID else { return }
        let peerId = central.identifier.uuidString
        if subscribedCentral?.identifier == central.identifier {
            subscribedCentral = nil
            pendingNotifyData = nil
        }
        onLinkClosed(peerIdentifier: peerId)
    }

    public func peripheralManager(
        _ peripheral: CBPeripheralManager,
        didReceiveWrite requests: [CBATTRequest]
    ) {
        for request in requests {
            if request.characteristic.uuid == Self.rxCharUUID, let data = request.value {
                handleReceivedData(data)
            }
            peripheral.respond(to: request, withResult: .success)
        }
    }

    /// Backpressure: the notify queue drained — retry the buffered frame.
    public func peripheralManagerIsReady(toUpdateSubscribers peripheral: CBPeripheralManager) {
        guard let data = pendingNotifyData, let tx = txCharacteristic else { return }
        pendingNotifyData = nil
        if !peripheral.updateValue(data, for: tx, onSubscribedCentrals: nil) {
            pendingNotifyData = data
        }
    }
}
