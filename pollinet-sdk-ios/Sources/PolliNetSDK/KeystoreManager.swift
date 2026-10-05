//
//  KeystoreManager.swift
//  Keychain / Secure Enclave counterpart of Android's KeystoreManager.kt.
//
//  Same caveat as Android, verbatim: hardware-backed keys are P-256 ECDSA,
//  NOT Ed25519 — Solana intents must be signed by a wallet-provided Ed25519
//  key. These enclave keys are for device identity / attestation only.
//

import CryptoKit
import Foundation
import Security

public final class KeystoreManager: Sendable {

    public static let shared = KeystoreManager()

    private let service = "xyz.pollinet.keys"

    public init() {}

    /// True when a Secure Enclave is present (Android: hasStrongBox).
    public static var hasSecureEnclave: Bool {
        SecureEnclave.isAvailable
    }

    /// Generate (or replace) a P-256 signing key under `alias`.
    /// Uses the Secure Enclave when available, else a software key — in both
    /// cases the persistable representation is stored in the Keychain.
    @discardableResult
    public func generateKeyPair(alias: String) throws -> Data {
        try deleteKey(alias: alias)
        let keyData: Data
        if SecureEnclave.isAvailable {
            let key = try SecureEnclave.P256.Signing.PrivateKey()
            keyData = key.dataRepresentation
        } else {
            keyData = P256.Signing.PrivateKey().rawRepresentation
        }
        var query = baseQuery(alias: alias)
        query[kSecValueData as String] = keyData
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw PolliNetError(code: "ERR_KEYSTORE", message: "Keychain add failed: \(status)")
        }
        return try publicKey(alias: alias)
    }

    /// ECDSA P-256 signature over `data` (DER encoding).
    public func sign(alias: String, data: Data) throws -> Data {
        let stored = try loadKeyData(alias: alias)
        if SecureEnclave.isAvailable,
           let key = try? SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: stored) {
            return try key.signature(for: data).derRepresentation
        }
        let key = try P256.Signing.PrivateKey(rawRepresentation: stored)
        return try key.signature(for: data).derRepresentation
    }

    /// X9.63 public key bytes for `alias`.
    public func publicKey(alias: String) throws -> Data {
        let stored = try loadKeyData(alias: alias)
        if SecureEnclave.isAvailable,
           let key = try? SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: stored) {
            return key.publicKey.x963Representation
        }
        let key = try P256.Signing.PrivateKey(rawRepresentation: stored)
        return key.publicKey.x963Representation
    }

    public func keyExists(alias: String) -> Bool {
        (try? loadKeyData(alias: alias)) != nil
    }

    @discardableResult
    public func deleteKey(alias: String) throws -> Bool {
        let status = SecItemDelete(baseQuery(alias: alias) as CFDictionary)
        switch status {
        case errSecSuccess: return true
        case errSecItemNotFound: return false
        default:
            throw PolliNetError(code: "ERR_KEYSTORE", message: "Keychain delete failed: \(status)")
        }
    }

    public func listKeys() -> [String] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecMatchLimit as String: kSecMatchLimitAll,
            kSecReturnAttributes as String: true,
        ]
        #if os(macOS)
        query[kSecUseDataProtectionKeychain as String] = true
        #endif
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let items = result as? [[String: Any]] else { return [] }
        return items.compactMap { $0[kSecAttrAccount as String] as? String }
    }

    // MARK: - Private

    private func baseQuery(alias: String) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: alias,
        ]
        #if os(macOS)
        query[kSecUseDataProtectionKeychain as String] = true
        #endif
        return query
    }

    private func loadKeyData(alias: String) throws -> Data {
        var query = baseQuery(alias: alias)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else {
            throw PolliNetError(code: "ERR_KEYSTORE", message: "Key '\(alias)' not found (\(status))")
        }
        return data
    }
}
