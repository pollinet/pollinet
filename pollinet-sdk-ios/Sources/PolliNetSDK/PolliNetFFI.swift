//
//  PolliNetFFI.swift
//  Low-level plumbing over the Rust C ABI (PolliNetRust.xcframework).
//
//  Counterpart of Android's PolliNetFFI.kt: every value crossing the boundary
//  is either a raw byte buffer or the JSON FfiResult envelope
//  ({"ok":true,"data":…} | {"ok":false,"code":…,"message":…}).
//

import Foundation
import PolliNetRust

/// Error surfaced by any PolliNet SDK call — mirrors Android's PolliNetException.
public struct PolliNetError: Error, LocalizedError, Sendable {
    public let code: String
    public let message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }

    public var errorDescription: String? { "\(code): \(message)" }
}

/// Internal helpers for crossing the C boundary.
enum FFI {
    /// Take ownership of a Rust-allocated C string and free it.
    static func consume(_ ptr: UnsafeMutablePointer<CChar>?) -> String {
        guard let ptr else { return "" }
        defer { pollinet_string_free(ptr) }
        return String(cString: ptr)
    }

    /// The untagged FfiResult envelope. `data` stays raw JSON so each call site
    /// decodes exactly the type it expects.
    private struct RawEnvelope: Decodable {
        let ok: Bool
        let code: String?
        let message: String?
    }

    /// Decode the envelope, throwing PolliNetError for {"ok":false,…}.
    /// Returns the raw JSON of the whole envelope for a second, typed pass.
    private static func check(_ json: String) throws -> Data {
        guard let data = json.data(using: .utf8) else {
            throw PolliNetError(code: "ERR_DECODE", message: "FFI returned non-UTF8 payload")
        }
        let envelope: RawEnvelope
        do {
            envelope = try JSONDecoder().decode(RawEnvelope.self, from: data)
        } catch {
            throw PolliNetError(code: "ERR_DECODE", message: "Malformed FFI envelope: \(json)")
        }
        guard envelope.ok else {
            throw PolliNetError(
                code: envelope.code ?? "ERR_UNKNOWN",
                message: envelope.message ?? "Unknown FFI error"
            )
        }
        return data
    }

    private struct DataEnvelope<T: Decodable>: Decodable {
        let data: T?
    }

    /// Decode a success envelope whose `data` may legitimately be null
    /// (queue pops, optional lookups).
    static func decodeOptional<T: Decodable>(_ type: T.Type, from json: String) throws -> T? {
        let data = try check(json)
        return try JSONDecoder().decode(DataEnvelope<T>.self, from: data).data
    }

    /// Decode a success envelope that must carry `data`.
    static func decode<T: Decodable>(_ type: T.Type, from json: String) throws -> T {
        guard let value = try decodeOptional(type, from: json) else {
            throw PolliNetError(code: "ERR_DECODE", message: "FFI envelope missing data: \(json)")
        }
        return value
    }

    /// Validate a success envelope, discarding `data` (unit-returning ops).
    static func decodeVoid(from json: String) throws {
        _ = try check(json)
    }

    /// Run `body` with (pointer, length) for a byte buffer; handles empty Data.
    /// Length is `UInt` to match the C ABI's `uintptr_t`.
    static func withBytes<R>(_ data: Data, _ body: (UnsafePointer<UInt8>?, UInt) -> R) -> R {
        if data.isEmpty {
            return body(nil, 0)
        }
        return data.withUnsafeBytes { raw in
            body(raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count))
        }
    }

    /// Encode an Encodable request as a NUL-terminated JSON string and run `body`.
    static func withJSON<T: Encodable, R>(_ value: T, _ body: (UnsafePointer<CChar>) throws -> R) throws -> R {
        let data = try JSONEncoder().encode(value)
        guard let json = String(data: data, encoding: .utf8) else {
            throw PolliNetError(code: "ERR_ENCODE", message: "Request is not valid UTF-8")
        }
        return try json.withCString(body)
    }

    /// Run the blocking FFI call off the caller's actor (counterpart of
    /// Kotlin's withContext(Dispatchers.IO) — every Rust op may block_on).
    static func run<T: Sendable>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    continuation.resume(returning: try body())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }
}
