//
//  Models.swift
//  Public data types for the PolliNet iOS SDK.
//
//  Field names mirror the Rust FFI wire format exactly (the same JSON the
//  Kotlin SDK decodes) — do not rename coding keys without moving the Rust
//  core and the Android SDK together.
//

import Foundation

// =============================================================================
// Configuration
// =============================================================================

/// SDK configuration passed to `PolliNetSDK.initialize`.
public struct SdkConfig: Codable, Sendable {
    public var version: Int
    public var rpcUrl: String?
    public var enableLogging: Bool
    public var logLevel: String?
    public var storageDirectory: String?
    /// AES-256-GCM encryption key for queue/bundle storage (any string; hashed internally).
    public var encryptionKey: String?
    /// Base58 wallet address owning this node session (reward attribution).
    public var walletAddress: String?

    public init(
        version: Int = 1,
        rpcUrl: String? = nil,
        enableLogging: Bool = true,
        logLevel: String? = "info",
        storageDirectory: String? = nil,
        encryptionKey: String? = nil,
        walletAddress: String? = nil
    ) {
        self.version = version
        self.rpcUrl = rpcUrl
        self.enableLogging = enableLogging
        self.logLevel = logLevel
        self.storageDirectory = storageDirectory
        self.encryptionKey = encryptionKey
        self.walletAddress = walletAddress
    }
}

// =============================================================================
// Fragmentation
// =============================================================================

public struct Fragment: Codable, Sendable {
    public let id: String
    public let index: Int
    public let total: Int
    /// base64 payload
    public let data: String
    public let fragmentType: String
    /// base64 checksum
    public let checksum: String

    enum CodingKeys: String, CodingKey {
        case id, index, total, data, checksum
        case fragmentType = "fragment_type"
    }
}

public struct FragmentList: Codable, Sendable {
    public let fragments: [Fragment]
}

/// Input fragment for reconstruction / outbound queueing (hex tx id + base64 data).
public struct FragmentData: Codable, Sendable {
    public let transactionId: String
    public let fragmentIndex: Int
    public let totalFragments: Int
    public let dataBase64: String

    public init(transactionId: String, fragmentIndex: Int, totalFragments: Int, dataBase64: String) {
        self.transactionId = transactionId
        self.fragmentIndex = fragmentIndex
        self.totalFragments = totalFragments
        self.dataBase64 = dataBase64
    }
}

public struct FragmentationStats: Codable, Sendable {
    public let originalSize: Int
    public let fragmentCount: Int
    public let maxFragmentSize: Int
    public let avgFragmentSize: Int
    public let totalOverhead: Int
    public let efficiency: Float
}

public struct FragmentPacket: Codable, Sendable {
    public let transactionId: String
    public let fragmentIndex: Int
    public let totalFragments: Int
    /// Base64-encoded mesh packet
    public let packetBytes: String
}

public struct BroadcastPreparation: Codable, Sendable {
    public let transactionId: String
    public let fragmentPackets: [FragmentPacket]
}

// =============================================================================
// Transport / metrics
// =============================================================================

public struct MetricsSnapshot: Codable, Sendable {
    public let fragmentsBuffered: Int
    public let transactionsComplete: Int
    public let reassemblyFailures: Int
    public let lastError: String
    public let updatedAt: Int64
}

public struct FragmentReassemblyInfo: Codable, Sendable {
    public let transactionId: String
    public let totalFragments: Int
    public let receivedFragments: Int
    public let receivedIndices: [Int]
    public let fragmentSizes: [Int]
    public let totalBytesReceived: Int
}

public struct FragmentReassemblyInfoList: Codable, Sendable {
    public let transactions: [FragmentReassemblyInfo]
}

public struct PushResponse: Codable, Sendable {
    public let added: Bool
    public let queueSize: Int

    enum CodingKeys: String, CodingKey {
        case added
        case queueSize = "queue_size"
    }
}

public struct ReceivedTransaction: Codable, Sendable {
    public let txId: String
    public let transactionBase64: String
    public let receivedAt: Int64
}

struct QueueSizeResponse: Codable {
    let queueSize: Int
}

struct SuccessResponse: Codable {
    let success: Bool
}

struct WalletAddressResponse: Codable {
    let address: String
}

public struct OutboundQueueDebug: Codable, Sendable {
    public let totalFragments: Int
    public let fragments: [FragmentDebugInfo]

    enum CodingKeys: String, CodingKey {
        case totalFragments = "total_fragments"
        case fragments
    }
}

public struct FragmentDebugInfo: Codable, Sendable {
    public let index: Int
    public let size: Int
}

// =============================================================================
// Queues
// =============================================================================

public enum Priority: String, Codable, Sendable {
    case high = "HIGH"
    case normal = "NORMAL"
    case low = "LOW"
}

public struct OutboundTransaction: Codable, Sendable {
    public let txId: String
    /// base64
    public let originalBytes: String
    public let fragmentCount: Int
    public let priority: Priority
    public let createdAt: Int64
    public let retryCount: Int
}

public struct RetryItem: Codable, Sendable {
    /// base64
    public let txBytes: String
    public let txId: String
    public let attemptCount: Int
    public let lastError: String
    public let nextRetryInSecs: Int64
    public let ageSeconds: Int64
}

/// Confirmation status — wire format is {"type":"SUCCESS","signature":…} or
/// {"type":"FAILED","error":…} (Rust ConfirmationStatusFFI).
public enum ConfirmationStatus: Codable, Sendable, Equatable {
    case success(signature: String)
    case failed(error: String)

    private enum CodingKeys: String, CodingKey {
        case type, signature, error
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(String.self, forKey: .type) {
        case "SUCCESS":
            self = .success(signature: try container.decode(String.self, forKey: .signature))
        case "FAILED":
            self = .failed(error: try container.decode(String.self, forKey: .error))
        case let other:
            throw DecodingError.dataCorruptedError(
                forKey: .type, in: container,
                debugDescription: "Unknown confirmation status type: \(other)"
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .success(let signature):
            try container.encode("SUCCESS", forKey: .type)
            try container.encode(signature, forKey: .signature)
        case .failed(let error):
            try container.encode("FAILED", forKey: .type)
            try container.encode(error, forKey: .error)
        }
    }
}

public struct Confirmation: Codable, Sendable, Equatable {
    /// hex tx id (32 bytes / 64 chars)
    public let txId: String
    public let status: ConfirmationStatus
    public let timestamp: Int64
    public let relayCount: Int

    public init(txId: String, status: ConfirmationStatus, timestamp: Int64, relayCount: Int) {
        self.txId = txId
        self.status = status
        self.timestamp = timestamp
        self.relayCount = relayCount
    }
}

/// Fragment descriptor used when pushing pre-fragmented transactions outbound
/// (hex tx id + base64 data). Mirrors Kotlin's FragmentFFI.
public struct FragmentFFI: Codable, Sendable {
    public let transactionId: String
    public let fragmentIndex: Int
    public let totalFragments: Int
    public let dataBase64: String

    public init(transactionId: String, fragmentIndex: Int, totalFragments: Int, dataBase64: String) {
        self.transactionId = transactionId
        self.fragmentIndex = fragmentIndex
        self.totalFragments = totalFragments
        self.dataBase64 = dataBase64
    }
}

struct PushOutboundRequest: Codable {
    var version: Int = 1
    let txBytes: String
    let txId: String
    let fragments: [FragmentFFI]
    let priority: Priority
}

struct AcceptExternalTransactionRequest: Codable {
    var version: Int = 1
    let base64SignedTx: String
    let maxPayload: Int?
}

struct AddToRetryRequest: Codable {
    var version: Int = 1
    let txBytes: String
    let txId: String
    let error: String
}

struct QueueConfirmationRequest: Codable {
    var version: Int = 1
    let txId: String
    let signature: String
}

/// Result of `loadForSending`.
public struct LoadForSendingResult: Codable, Sendable {
    public let txId: String
    public let relevance: Int
    public let fragmentCount: Int

    enum CodingKeys: String, CodingKey {
        case txId = "tx_id"
        case relevance
        case fragmentCount = "fragment_count"
    }
}

// =============================================================================
// Mesh health
// =============================================================================

public enum PeerState: String, Codable, Sendable {
    case connected = "Connected"
    case stale = "Stale"
    case dead = "Dead"
}

public struct PeerHealth: Codable, Sendable {
    public let peerId: String
    public let state: PeerState
    public let secondsSinceLastSeen: Int64
    public let latencySamples: [Int]
    public let avgLatencyMs: Int
    public let rssi: Int?
    public let qualityScore: Int
    public let packetsSent: Int64
    public let packetsReceived: Int64
    public let txFailures: Int64
    public let packetLossRate: Float

    enum CodingKeys: String, CodingKey {
        case peerId = "peer_id"
        case state
        case secondsSinceLastSeen = "seconds_since_last_seen"
        case latencySamples = "latency_samples"
        case avgLatencyMs = "avg_latency_ms"
        case rssi
        case qualityScore = "quality_score"
        case packetsSent = "packets_sent"
        case packetsReceived = "packets_received"
        case txFailures = "tx_failures"
        case packetLossRate = "packet_loss_rate"
    }
}

public struct NetworkTopology: Codable, Sendable {
    public let directConnections: [String]
    public let allPeers: [String]
    public let connections: [String: [String]]
    public let hopCounts: [String: Int]

    enum CodingKeys: String, CodingKey {
        case directConnections = "direct_connections"
        case allPeers = "all_peers"
        case connections
        case hopCounts = "hop_counts"
    }
}

public struct HealthMetrics: Codable, Sendable {
    public let totalPeers: Int
    public let connectedPeers: Int
    public let stalePeers: Int
    public let deadPeers: Int
    public let avgLatencyMs: Int
    public let maxLatencyMs: Int
    public let minLatencyMs: Int
    public let avgPacketLoss: Float
    public let healthScore: Int
    public let maxHops: Int
    public let timestamp: String

    enum CodingKeys: String, CodingKey {
        case totalPeers = "total_peers"
        case connectedPeers = "connected_peers"
        case stalePeers = "stale_peers"
        case deadPeers = "dead_peers"
        case avgLatencyMs = "avg_latency_ms"
        case maxLatencyMs = "max_latency_ms"
        case minLatencyMs = "min_latency_ms"
        case avgPacketLoss = "avg_packet_loss"
        case healthScore = "health_score"
        case maxHops = "max_hops"
        case timestamp
    }
}

public struct HealthSnapshot: Codable, Sendable {
    public let peers: [PeerHealth]
    public let topology: NetworkTopology
    public let metrics: HealthMetrics

    /// Peers currently in the Connected state.
    public var connectedPeers: [PeerHealth] { peers.filter { $0.state == .connected } }
    /// All known peer addresses (connected + stale + dead).
    public var knownPeerIds: [String] { peers.map(\.peerId) }
}

// =============================================================================
// Intent protocol
// =============================================================================

/// One token account to grant delegate authority in `createApproveTransaction`.
public struct TokenApprovalEntry: Codable, Sendable {
    public let mintAddress: String
    public let amount: Int64
    public let decimals: Int
    /// Owner's token account for this mint (ATA or custom).
    public let tokenAccount: String
    /// "spl-token" (default) or "token-2022".
    public let tokenProgram: String

    enum CodingKeys: String, CodingKey {
        case mintAddress = "mint_address"
        case amount, decimals
        case tokenAccount = "token_account"
        case tokenProgram = "token_program"
    }

    public init(
        mintAddress: String,
        amount: Int64,
        decimals: Int = 6,
        tokenAccount: String,
        tokenProgram: String = "spl-token"
    ) {
        self.mintAddress = mintAddress
        self.amount = amount
        self.decimals = decimals
        self.tokenAccount = tokenAccount
        self.tokenProgram = tokenProgram
    }
}

public struct ApproveTransactionResponse: Codable, Sendable {
    /// Base64-encoded unsigned transaction; sign with owner wallet before submitting.
    public let transaction: String
    /// The executor PDA that was granted delegate authority.
    public let executorPda: String

    enum CodingKeys: String, CodingKey {
        case transaction
        case executorPda = "executor_pda"
    }
}

public struct ExecutorPdaResponse: Codable, Sendable {
    public let pda: String
    public let bump: Int
}

/// Snapshot of one SPL token account + its delegation status vs the executor PDA.
public struct DelegatedTokenAccount: Sendable {
    public let pubkey: String
    public let mint: String
    public let owner: String
    public let decimals: Int
    public let rawBalance: Int64
    public let delegate: String?
    public let delegatedRawAmount: Int64
    /// true iff delegate == executor PDA AND delegatedRawAmount > 0.
    public let isExecutorDelegated: Bool
}

struct CreateIntentBytesRequest: Codable {
    let from: String
    let to: String
    let tokenMint: String
    let amount: Int64
    let expiresAt: Int64
    let gasFeeAmount: Int64
    let gasFeePayee: String
    let nonceHex: String?

    enum CodingKeys: String, CodingKey {
        case from, to, amount
        case tokenMint = "token_mint"
        case expiresAt = "expires_at"
        case gasFeeAmount = "gas_fee_amount"
        case gasFeePayee = "gas_fee_payee"
        case nonceHex = "nonce_hex"
    }
}

public struct IntentBytesResponse: Codable, Sendable {
    /// Base64-encoded 169-byte intent — sign this with Ed25519 before submitting.
    public let intentBytes: String
    /// The 16-byte nonce used (32 lowercase hex chars).
    public let nonceHex: String

    enum CodingKeys: String, CodingKey {
        case intentBytes = "intent_bytes"
        case nonceHex = "nonce_hex"
    }
}

struct CreateApproveTransactionRequest: Codable {
    let ownerWallet: String
    let feePayer: String
    let recentBlockhash: String
    let tokens: [TokenApprovalEntry]

    enum CodingKeys: String, CodingKey {
        case ownerWallet = "owner_wallet"
        case feePayer = "fee_payer"
        case recentBlockhash = "recent_blockhash"
        case tokens
    }
}

struct CreateRevokeTransactionRequest: Codable {
    let ownerWallet: String
    let feePayer: String
    let recentBlockhash: String
    let tokenAccounts: [String]
    let tokenProgram: String

    enum CodingKeys: String, CodingKey {
        case ownerWallet = "owner_wallet"
        case feePayer = "fee_payer"
        case recentBlockhash = "recent_blockhash"
        case tokenAccounts = "token_accounts"
        case tokenProgram = "token_program"
    }
}

struct RevokeTransactionResponse: Codable {
    let transaction: String
}

public struct IntentStateResponse: Codable, Sendable {
    public let initialized: Bool
    public let user: String
    public let pda: String
    public let totalExecuted: String?

    enum CodingKeys: String, CodingKey {
        case initialized, user, pda
        case totalExecuted = "total_executed"
    }
}

public struct InitTxResponse: Codable, Sendable {
    /// Base64-encoded partially-signed transaction. Sign with the user's wallet.
    public let tx: String
    public let user: String
}

public struct InitializeResponse: Codable, Sendable {
    public let ok: Bool
    public let txSignature: String

    enum CodingKeys: String, CodingKey {
        case ok
        case txSignature = "tx_signature"
    }
}

struct InitializeRequest: Codable {
    let tx: String
    let wallet: String
}

struct GatewayResponse: Codable {
    let wallet: String
}

struct SubmitIntentFFIRequest: Codable {
    let intentBytes: String
    let signature: String
    let fromTokenAccount: String
    let tokenProgram: String

    enum CodingKeys: String, CodingKey {
        case intentBytes = "intent_bytes"
        case signature
        case fromTokenAccount = "from_token_account"
        case tokenProgram = "token_program"
    }
}

struct SubmitIntentFFIResponse: Codable {
    let ok: Bool
    let txSignature: String

    enum CodingKeys: String, CodingKey {
        case ok
        case txSignature = "tx_signature"
    }
}

// =============================================================================
// Subsystem 1 — density-adaptive rotation
// =============================================================================

public struct AdaptiveParams: Codable, Sendable {
    /// Estimated unique peers observed in the last 2 minutes.
    public let density: Int
    /// Target session duration in ms. Clamped [20_000, 120_000].
    public let sessionTargetMs: Int64
    /// Peer cooldown duration in ms. Clamped [15_000, 600_000].
    public let cooldownMs: Int64
    public let sessionMinMs: Int64
    public let sessionMaxMs: Int64

    enum CodingKeys: String, CodingKey {
        case density
        case sessionTargetMs = "session_target_ms"
        case cooldownMs = "cooldown_ms"
        case sessionMinMs = "session_min_ms"
        case sessionMaxMs = "session_max_ms"
    }
}

/// Session telemetry record logged after each radio session.
public struct SessionTelemetryRecord: Codable, Sendable {
    public let localDeviceId: String
    public let peerId: String
    public let connectTimeMs: Int64
    public let disconnectTimeMs: Int64
    public let bytesOut: Int64
    public let bytesIn: Int64
    public let fragmentsOut: Int
    public let fragmentsIn: Int
    public let dataComplete: Bool
    public let confirmationComplete: Bool
    public let closeReason: String

    enum CodingKeys: String, CodingKey {
        case localDeviceId = "local_device_id"
        case peerId = "peer_id"
        case connectTimeMs = "connect_time_ms"
        case disconnectTimeMs = "disconnect_time_ms"
        case bytesOut = "bytes_out"
        case bytesIn = "bytes_in"
        case fragmentsOut = "fragments_out"
        case fragmentsIn = "fragments_in"
        case dataComplete = "data_complete"
        case confirmationComplete = "confirmation_complete"
        case closeReason = "close_reason"
    }

    public init(
        localDeviceId: String, peerId: String,
        connectTimeMs: Int64, disconnectTimeMs: Int64,
        bytesOut: Int64, bytesIn: Int64,
        fragmentsOut: Int, fragmentsIn: Int,
        dataComplete: Bool, confirmationComplete: Bool,
        closeReason: String
    ) {
        self.localDeviceId = localDeviceId
        self.peerId = peerId
        self.connectTimeMs = connectTimeMs
        self.disconnectTimeMs = disconnectTimeMs
        self.bytesOut = bytesOut
        self.bytesIn = bytesIn
        self.fragmentsOut = fragmentsOut
        self.fragmentsIn = fragmentsIn
        self.dataComplete = dataComplete
        self.confirmationComplete = confirmationComplete
        self.closeReason = closeReason
    }
}

// =============================================================================
// Subsystem 3 — confirmation-driven purge
// =============================================================================

/// Result of `ingestConfirmation`.
public struct IngestConfirmationResult: Codable, Sendable {
    /// True if a matching carrier entry was found and removed.
    public let purged: Bool
    /// True if the confirmation was added to the carrier set for re-propagation.
    public let addedToCarrier: Bool

    enum CodingKeys: String, CodingKey {
        case purged
        case addedToCarrier = "added_to_carrier"
    }
}
