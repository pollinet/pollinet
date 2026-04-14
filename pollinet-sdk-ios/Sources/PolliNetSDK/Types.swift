import Foundation

// MARK: - Received transaction

public struct ReceivedTransaction: Decodable {
    public let txId: String
    public let transactionBase64: String
    public let receivedAt: Int64
}

// MARK: - Retry

public struct RetryItem: Decodable {
    public let txBytes: String        // base64
    public let txId: String
    public let attemptCount: Int
    public let lastError: String
    public let nextRetryInSecs: Int64
    public let ageSeconds: Int64
}

// MARK: - Confirmation

public enum ConfirmationStatus: Codable {
    case success(signature: String)
    case failed(error: String)

    private enum CodingKeys: String, CodingKey { case type, signature, error }
    private enum StatusType: String, Codable { case SUCCESS, FAILED }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type_ = try c.decode(StatusType.self, forKey: .type)
        switch type_ {
        case .SUCCESS: self = .success(signature: try c.decode(String.self, forKey: .signature))
        case .FAILED:  self = .failed(error:     try c.decode(String.self, forKey: .error))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .success(let sig):
            try c.encode(StatusType.SUCCESS, forKey: .type)
            try c.encode(sig, forKey: .signature)
        case .failed(let err):
            try c.encode(StatusType.FAILED, forKey: .type)
            try c.encode(err, forKey: .error)
        }
    }
}

public struct Confirmation: Codable {
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

// MARK: - Fragmentation

public struct Fragment: Decodable {
    public let id: String
    public let index: Int
    public let total: Int
    public let data: String       // base64
    public let fragmentType: String
    public let checksum: String   // base64
}

public struct FragmentList: Decodable {
    public let fragments: [Fragment]
}

// MARK: - Queue metrics

public struct QueueMetrics: Decodable {
    public let outboundSize: Int
    public let outboundHighPriority: Int
    public let outboundNormalPriority: Int
    public let outboundLowPriority: Int
    public let confirmationSize: Int
    public let retrySize: Int
    public let retryAvgAttempts: Float
}
