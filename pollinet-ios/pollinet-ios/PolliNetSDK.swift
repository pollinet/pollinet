//
//  PolliNetSDK.swift
//  pollinet-ios
//
//  Swift wrapper for PolliNet Rust FFI
//

import Foundation

/// Main SDK class for interacting with PolliNet
public class PolliNetSDK {
    
    // MARK: - Properties
    
    private var handle: Int64 = -1
    private let version: String
    
    // MARK: - Initialization
    
    public init() {
        // Get SDK version
        if let versionPtr = pollinet_version() {
            self.version = String(cString: versionPtr)
            pollinet_free_string(versionPtr)
        } else {
            self.version = "unknown"
        }
    }
    
    /// Initialize the SDK with configuration
    /// - Parameter config: SDK configuration dictionary
    /// - Returns: True if initialization succeeded, false otherwise
    @discardableResult
    public func initialize(config: SdkConfig) -> Bool {
        guard let configJson = try? JSONEncoder().encode(config),
              let configString = String(data: configJson, encoding: .utf8) else {
            return false
        }
        
        let handle = pollinet_init(configString)
        if handle >= 0 {
            self.handle = handle
            return true
        }
        return false
    }
    
    /// Shutdown the SDK instance
    public func shutdown() {
        if handle >= 0 {
            pollinet_shutdown(handle)
            handle = -1
        }
    }
    
    /// Get SDK version
    public func getVersion() -> String {
        return version
    }
    
    // MARK: - Transport API
    
    /// Push inbound BLE data
    public func pushInbound(data: Data) -> Bool {
        guard handle >= 0 else { return false }
        return data.withUnsafeBytes { bytes in
            pollinet_push_inbound(handle, bytes.baseAddress?.assumingMemoryBound(to: UInt8.self), data.count) != 0
        }
    }
    
    /// Get next outbound data to send over BLE
    public func nextOutbound() -> Data? {
        guard handle >= 0 else { return nil }
        
        var outLen: Int = 0
        var buffer = [UInt8](repeating: 0, count: 1024) // Initial buffer size
        
        // First call to get size
        let result = buffer.withUnsafeMutableBytes { bytes in
            pollinet_next_outbound(handle, bytes.baseAddress?.assumingMemoryBound(to: UInt8.self), &outLen)
        }
        
        if result == 0 || outLen == 0 {
            return nil
        }
        
        // Resize buffer if needed
        if outLen > buffer.count {
            buffer = [UInt8](repeating: 0, count: outLen)
        }
        
        // Second call to get actual data
        let success = buffer.withUnsafeMutableBytes { bytes in
            pollinet_next_outbound(handle, bytes.baseAddress?.assumingMemoryBound(to: UInt8.self), &outLen)
        }
        
        if success != 0 && outLen > 0 {
            return Data(buffer.prefix(outLen))
        }
        
        return nil
    }
    
    /// Perform periodic tick (retry logic, timeouts)
    public func tick() {
        guard handle >= 0 else { return }
        pollinet_tick(handle)
    }
    
    /// Get current metrics
    public func getMetrics() -> Metrics? {
        guard handle >= 0 else { return nil }
        
        guard let jsonPtr = pollinet_metrics(handle) else { return nil }
        defer { pollinet_free_string(jsonPtr) }
        
        let jsonString = String(cString: jsonPtr)
        guard let jsonData = jsonString.data(using: .utf8),
              let metrics = try? JSONDecoder().decode(Metrics.self, from: jsonData) else {
            return nil
        }
        
        return metrics
    }
    
    /// Clear a transaction from internal buffers
    public func clearTransaction(txId: String) -> Bool {
        guard handle >= 0 else { return false }
        return pollinet_clear_transaction(handle, txId) != 0
    }
    
    // MARK: - Transaction Building
    
    /// Create an unsigned SOL transfer transaction
    public func createUnsignedTransaction(request: CreateUnsignedTransactionRequest) -> Result<String, Error> {
        guard handle >= 0 else {
            return .failure(PolliNetError.notInitialized)
        }
        
        return executeFFICall { jsonData in
            pollinet_create_unsigned_transaction(handle, jsonData.baseAddress, jsonData.count)
        } request: request
    }
    
    /// Create an unsigned SPL token transfer transaction
    public func createUnsignedSplTransaction(request: CreateUnsignedSplTransactionRequest) -> Result<String, Error> {
        guard handle >= 0 else {
            return .failure(PolliNetError.notInitialized)
        }
        
        return executeFFICall { jsonData in
            pollinet_create_unsigned_spl_transaction(handle, jsonData.baseAddress, jsonData.count)
        } request: request
    }
    
    // MARK: - Helper Methods
    
    /// Execute an FFI call that returns a JSON string
    private func executeFFICall<T: Encodable>(
        _ ffiCall: (UnsafeBufferPointer<UInt8>) -> UnsafeMutablePointer<CChar>?,
        request: T
    ) -> Result<String, Error> {
        guard let requestData = try? JSONEncoder().encode(request) else {
            return .failure(PolliNetError.serializationError)
        }
        
        guard let jsonPtr = requestData.withUnsafeBytes({ bytes in
            ffiCall(bytes)
        }) else {
            return .failure(PolliNetError.ffiCallFailed)
        }
        
        defer { pollinet_free_string(jsonPtr) }
        
        let jsonString = String(cString: jsonPtr)
        
        // Parse FfiResult
        guard let jsonData = jsonString.data(using: .utf8),
              let result = try? JSONDecoder().decode(FfiResult<String>.self, from: jsonData) else {
            return .failure(PolliNetError.deserializationError)
        }
        
        switch result {
        case .ok(let data):
            return .success(data)
        case .err(let error):
            return .failure(PolliNetError.ffiError(code: error.code, message: error.message))
        }
    }
}

// MARK: - Error Types

public enum PolliNetError: Error {
    case notInitialized
    case serializationError
    case deserializationError
    case ffiCallFailed
    case ffiError(code: String, message: String)
    
    public var localizedDescription: String {
        switch self {
        case .notInitialized:
            return "SDK not initialized. Call initialize() first."
        case .serializationError:
            return "Failed to serialize request"
        case .deserializationError:
            return "Failed to deserialize response"
        case .ffiCallFailed:
            return "FFI call returned null"
        case .ffiError(let code, let message):
            return "FFI Error [\(code)]: \(message)"
        }
    }
}

// MARK: - Data Models

public struct SdkConfig: Codable {
    public var version: UInt32 = 1
    public var rpcUrl: String?
    public var enableLogging: Bool = true
    public var logLevel: String?
    public var storageDirectory: String?
    
    public init(rpcUrl: String? = nil, enableLogging: Bool = true, logLevel: String? = nil, storageDirectory: String? = nil) {
        self.rpcUrl = rpcUrl
        self.enableLogging = enableLogging
        self.logLevel = logLevel
        self.storageDirectory = storageDirectory
    }
}

public struct CreateUnsignedTransactionRequest: Codable {
    public var version: UInt32 = 1
    public var sender: String
    public var recipient: String
    public var feePayer: String
    public var amount: UInt64
    public var nonceAccount: String?
    public var nonceData: CachedNonceData?
    
    public init(sender: String, recipient: String, feePayer: String, amount: UInt64, nonceAccount: String? = nil, nonceData: CachedNonceData? = nil) {
        self.sender = sender
        self.recipient = recipient
        self.feePayer = feePayer
        self.amount = amount
        self.nonceAccount = nonceAccount
        self.nonceData = nonceData
    }
}

public struct CreateUnsignedSplTransactionRequest: Codable {
    public var version: UInt32 = 1
    public var senderWallet: String
    public var recipientWallet: String
    public var feePayer: String
    public var mintAddress: String
    public var amount: UInt64
    public var nonceAccount: String?
    public var nonceData: CachedNonceData?
    
    public init(senderWallet: String, recipientWallet: String, feePayer: String, mintAddress: String, amount: UInt64, nonceAccount: String? = nil, nonceData: CachedNonceData? = nil) {
        self.senderWallet = senderWallet
        self.recipientWallet = recipientWallet
        self.feePayer = feePayer
        self.mintAddress = mintAddress
        self.amount = amount
        self.nonceAccount = nonceAccount
        self.nonceData = nonceData
    }
}

public struct CachedNonceData: Codable {
    public var version: UInt32 = 1
    public var nonceAccount: String
    public var authority: String
    public var blockhash: String
    public var lamportsPerSignature: UInt64
    public var cachedAt: UInt64
    public var used: Bool
}

public struct Metrics: Codable {
    // Add metrics fields based on your Rust Metrics struct
    // This is a placeholder - update with actual fields
}

// MARK: - FfiResult

enum FfiResult<T: Decodable>: Decodable {
    case ok(T)
    case err(FFIError)
    
    enum CodingKeys: String, CodingKey {
        case ok, data, code, message
    }
    
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let isOk = try container.decode(Bool.self, forKey: .ok)
        
        if isOk {
            let data = try container.decode(T.self, forKey: .data)
            self = .ok(data)
        } else {
            let code = try container.decode(String.self, forKey: .code)
            let message = try container.decode(String.self, forKey: .message)
            self = .err(FFIError(code: code, message: message))
        }
    }
}

struct FFIError: Decodable {
    let code: String
    let message: String
}
