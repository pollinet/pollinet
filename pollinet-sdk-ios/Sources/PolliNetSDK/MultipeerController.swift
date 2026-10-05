//
//  MultipeerController.swift
//  MultipeerConnectivity transport — the iOS counterpart of Android's
//  WifiDirectService.kt. iOS has no Wi-Fi Direct API; MPC provides the
//  high-bandwidth second radio for iOS↔iOS links (cross-OS traffic rides BLE).
//
//  Wire framing is IDENTICAL to the Wi-Fi Direct TCP framing so a future
//  cross-OS LAN/TCP bridge is drop-in:
//      [u32 BE length][1-byte type][payload]     length covers type + payload
//      type 0x00 = bincode TransactionFragment  → pushInbound
//      type 0x01 = JSON Confirmation            → confirmation handling + relay
//  Frames are DoS-guarded at MAX_FRAME (16 KiB); default payload target is
//  1400 B (WIFI_DIRECT_MAX_PAYLOAD, supplied by the Rust adapter's handle).
//
//  The controller must be given an SDK bound to a Wi-Fi/Multipeer handle —
//  usually `bleSdk.makeSharedWifiDirectSDK()` so both radios share one dedup
//  set and a transaction arriving over both is submitted exactly once.
//

import Combine
import Foundation
import MultipeerConnectivity

public enum MultipeerLinkStatus: String, Sendable {
    case idle, discovering, waiting, connected
}

public struct MultipeerConfirmationInfo: Sendable {
    public let txIdShort: String
    public let success: Bool
    public let detail: String
}

public final class MultipeerController: NSObject, ObservableObject, @unchecked Sendable {

    /// Bonjour service type (≤15 chars, lowercase + hyphen). Registered in the
    /// app's Info.plist under NSBonjourServices as `_pollinet-mesh._tcp`.
    public static let serviceType = "pollinet-mesh"

    // Wire constants — must match WifiDirectService.kt / the Rust adapter.
    static let frameTypeFragment: UInt8 = 0x00
    static let frameTypeConfirmation: UInt8 = 0x01
    static let maxFrame = 16 * 1024
    static let maxPayload = 1400
    private static let sendLoopInterval: TimeInterval = 0.3
    private static let maxRelayHops = 5

    private let sdk: PolliNetSDK
    private let peerID: MCPeerID
    private let session: MCSession
    private var advertiser: MCNearbyServiceAdvertiser?
    private var browser: MCNearbyServiceBrowser?
    private let queue = DispatchQueue(label: "xyz.pollinet.multipeer")
    private var sendLoopTask: Task<Void, Never>?
    private var seenConfirmationTxIds = Set<String>()

    @Published public private(set) var linkStatus: MultipeerLinkStatus = .idle
    @Published public private(set) var connectedPeers: Int = 0
    @Published public private(set) var logs: [String] = []

    private var confirmationContinuations: [UUID: AsyncStream<MultipeerConfirmationInfo>.Continuation] = [:]

    /// Stream of relayed confirmations (SharedFlow counterpart).
    public var confirmations: AsyncStream<MultipeerConfirmationInfo> {
        AsyncStream { continuation in
            let id = UUID()
            queue.async { self.confirmationContinuations[id] = continuation }
            continuation.onTermination = { [weak self] _ in
                self?.queue.async { self?.confirmationContinuations.removeValue(forKey: id) }
            }
        }
    }

    /// - Parameter sdk: an SDK bound to a Wi-Fi/Multipeer handle
    ///   (`bleSdk.makeSharedWifiDirectSDK()` for cross-transport dedup, or a
    ///   standalone `PolliNetSDK.initializeWifiDirect(config:)`).
    public init(sdk: PolliNetSDK, displayName: String = "PolliNet") {
        self.sdk = sdk
        self.peerID = MCPeerID(displayName: displayName)
        self.session = MCSession(peer: peerID, securityIdentity: nil, encryptionPreference: .required)
        super.init()
        session.delegate = self
    }

    // MARK: - Lifecycle

    /// Start advertising + browsing (roles are symmetric in MPC; the framework
    /// resolves who invites whom — the counterpart of P2P group formation).
    public func start() {
        queue.async {
            if self.advertiser == nil {
                let advertiser = MCNearbyServiceAdvertiser(
                    peer: self.peerID, discoveryInfo: nil, serviceType: Self.serviceType
                )
                advertiser.delegate = self
                self.advertiser = advertiser
            }
            if self.browser == nil {
                let browser = MCNearbyServiceBrowser(peer: self.peerID, serviceType: Self.serviceType)
                browser.delegate = self
                self.browser = browser
            }
            self.advertiser?.startAdvertisingPeer()
            self.browser?.startBrowsingForPeers()
            self.setStatus(.discovering)
            self.log("📶 Multipeer discovery started (\(Self.serviceType))")
        }
        startSendLoop()
    }

    public func stop() {
        sendLoopTask?.cancel()
        sendLoopTask = nil
        queue.async {
            self.advertiser?.stopAdvertisingPeer()
            self.browser?.stopBrowsingForPeers()
            self.session.disconnect()
            self.setStatus(.idle)
            self.log("🛑 Multipeer stopped")
        }
    }

    // MARK: - Send pump

    private func startSendLoop() {
        guard sendLoopTask == nil else { return }
        sendLoopTask = Task.detached { [weak self] in
            while !Task.isCancelled {
                await self?.pumpOutbound()
                try? await Task.sleep(nanoseconds: UInt64(Self.sendLoopInterval * 1_000_000_000))
            }
        }
    }

    private func pumpOutbound() async {
        guard !session.connectedPeers.isEmpty else { return }

        // Data fragments (bincode) — the adapter's 1400 B payload cap applies.
        while let fragment = await sdk.nextOutbound(maxLen: Self.maxPayload) {
            send(frameType: Self.frameTypeFragment, payload: fragment)
        }

        // Confirmation reverse channel (JSON, frame type 0x01).
        for _ in 0..<10 {
            guard let confirmation = try? await sdk.popConfirmation() else { break }
            guard let json = try? JSONEncoder().encode(confirmation) else { continue }
            send(frameType: Self.frameTypeConfirmation, payload: json)
        }
    }

    private func send(frameType: UInt8, payload: Data) {
        let length = payload.count + 1
        guard length <= Self.maxFrame else {
            log("❌ Frame \(length)B exceeds MAX_FRAME — dropped")
            return
        }
        var frame = Data(capacity: 4 + length)
        frame.append(UInt8((length >> 24) & 0xFF))
        frame.append(UInt8((length >> 16) & 0xFF))
        frame.append(UInt8((length >> 8) & 0xFF))
        frame.append(UInt8(length & 0xFF))
        frame.append(frameType)
        frame.append(payload)
        do {
            try session.send(frame, toPeers: session.connectedPeers, with: .reliable)
        } catch {
            log("❌ Multipeer send failed: \(error.localizedDescription)")
        }
    }

    // MARK: - Receive path

    /// An MPC message may carry one or more complete frames (never partial —
    /// MPC is message-oriented; the length prefix is kept for wire parity).
    private func handleReceived(_ data: Data) {
        var offset = data.startIndex
        while data.distance(from: offset, to: data.endIndex) >= 5 {
            let length = Int(data[offset]) << 24
                | Int(data[data.index(offset, offsetBy: 1)]) << 16
                | Int(data[data.index(offset, offsetBy: 2)]) << 8
                | Int(data[data.index(offset, offsetBy: 3)])
            guard length >= 1, length <= Self.maxFrame else {
                log("❌ Bad frame length \(length) — dropping message")
                return
            }
            let typeIndex = data.index(offset, offsetBy: 4)
            guard data.distance(from: typeIndex, to: data.endIndex) >= length else {
                log("❌ Truncated frame (declared \(length)B) — dropping message")
                return
            }
            let frameType = data[typeIndex]
            let payloadStart = data.index(after: typeIndex)
            let payloadEnd = data.index(typeIndex, offsetBy: length)
            let payload = Data(data[payloadStart..<payloadEnd])

            switch frameType {
            case Self.frameTypeFragment:
                Task { [weak self] in
                    try? await self?.sdk.pushInbound(payload)
                }
            case Self.frameTypeConfirmation:
                handleConfirmation(payload)
            default:
                log("⚠️ Unknown frame type 0x\(String(frameType, radix: 16)) — both peers must run ≥ framing version")
            }
            offset = payloadEnd
        }
    }

    private func handleConfirmation(_ json: Data) {
        guard let confirmation = try? JSONDecoder().decode(Confirmation.self, from: json) else {
            log("❌ Failed to decode confirmation JSON (\(json.count)B)")
            return
        }
        guard seenConfirmationTxIds.insert(confirmation.txId).inserted else { return }

        let txShort = String(confirmation.txId.prefix(8))
        let info: MultipeerConfirmationInfo
        switch confirmation.status {
        case .success(let signature):
            info = MultipeerConfirmationInfo(txIdShort: txShort, success: true, detail: String(signature.prefix(16)))
        case .failed(let error):
            info = MultipeerConfirmationInfo(txIdShort: txShort, success: false, detail: error)
        }
        queue.async {
            for continuation in self.confirmationContinuations.values {
                continuation.yield(info)
            }
        }
        if confirmation.relayCount < Self.maxRelayHops {
            Task { [weak self] in
                try? await self?.sdk.relayConfirmation(confirmation)
            }
        }
    }

    // MARK: - Helpers

    private func setStatus(_ status: MultipeerLinkStatus) {
        DispatchQueue.main.async { self.linkStatus = status }
    }

    private func log(_ line: String) {
        print("[PolliNet.MPC] \(line)") // Xcode console (logcat counterpart)
        DispatchQueue.main.async {
            self.logs.append(line)
            if self.logs.count > 200 { self.logs.removeFirst(self.logs.count - 200) }
        }
    }
}

// MARK: - MCSessionDelegate

extension MultipeerController: MCSessionDelegate {

    public func session(_ session: MCSession, peer peerID: MCPeerID, didChange state: MCSessionState) {
        let count = session.connectedPeers.count
        DispatchQueue.main.async { self.connectedPeers = count }
        switch state {
        case .connected:
            setStatus(.connected)
            log("🔗 Multipeer connected: \(peerID.displayName) (\(count) peer(s))")
        case .connecting:
            setStatus(.waiting)
        case .notConnected:
            setStatus(count > 0 ? .connected : .discovering)
            log("🔌 Multipeer disconnected: \(peerID.displayName)")
        @unknown default:
            break
        }
    }

    public func session(_ session: MCSession, didReceive data: Data, fromPeer peerID: MCPeerID) {
        handleReceived(data)
    }

    public func session(
        _ session: MCSession, didReceive stream: InputStream,
        withName streamName: String, fromPeer peerID: MCPeerID
    ) {}

    public func session(
        _ session: MCSession, didStartReceivingResourceWithName resourceName: String,
        fromPeer peerID: MCPeerID, with progress: Progress
    ) {}

    public func session(
        _ session: MCSession, didFinishReceivingResourceWithName resourceName: String,
        fromPeer peerID: MCPeerID, at localURL: URL?, withError error: Error?
    ) {}
}

// MARK: - Advertiser / browser delegates (symmetric auto-join)

extension MultipeerController: MCNearbyServiceAdvertiserDelegate {
    public func advertiser(
        _ advertiser: MCNearbyServiceAdvertiser,
        didReceiveInvitationFromPeer peerID: MCPeerID,
        withContext context: Data?,
        invitationHandler: @escaping (Bool, MCSession?) -> Void
    ) {
        log("🤝 Invitation from \(peerID.displayName) — accepting")
        invitationHandler(true, session)
    }
}

extension MultipeerController: MCNearbyServiceBrowserDelegate {
    public func browser(
        _ browser: MCNearbyServiceBrowser,
        foundPeer peerID: MCPeerID,
        withDiscoveryInfo info: [String: String]?
    ) {
        // Deterministic invite direction (lexicographic) so both sides don't
        // race to invite each other — the counterpart of P2P role election.
        guard self.peerID.displayName > peerID.displayName
            || (self.peerID.displayName == peerID.displayName && self.peerID.hashValue > peerID.hashValue)
        else { return }
        log("🔍 Found \(peerID.displayName) — inviting")
        browser.invitePeer(peerID, to: session, withContext: nil, timeout: 15)
    }

    public func browser(_ browser: MCNearbyServiceBrowser, lostPeer peerID: MCPeerID) {
        log("👋 Lost \(peerID.displayName)")
    }
}
