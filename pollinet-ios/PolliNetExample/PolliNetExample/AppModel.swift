//
//  AppModel.swift
//  App-wide state: SDK lifecycle, transports, and the send/approve flows.
//
//  QA signing: a locally generated Ed25519 key (persisted in UserDefaults,
//  DEV ONLY) signs intents so the offline flow works end-to-end without an
//  external wallet — mirroring Pollistem's QA setup. Production apps should
//  sign with the user's real wallet key.
//

import Combine
import CryptoKit
import Foundation
import PolliNetSDK
import SwiftUI

@MainActor
final class AppModel: ObservableObject {

    /// BGTask handlers need the SDK before/without UI — kept in sync with `sdk`.
    nonisolated(unsafe) static var sharedSDK: PolliNetSDK?

    // Config
    @AppStorage("rpcUrl") var rpcUrl = "https://api.devnet.solana.com"
    @AppStorage("tokenMint") var tokenMint = ""

    // Runtime
    @Published var sdk: PolliNetSDK?
    @Published var ble: BleController?
    @Published var multipeer: MultipeerController?
    @Published var status = "Not initialized"
    @Published var tokenAccounts: [DelegatedTokenAccount] = []
    @Published var lastError: String?

    // Dev wallet (Ed25519)
    @Published private(set) var walletAddress = ""
    private var signingKey: Curve25519.Signing.PrivateKey?

    init() {
        loadOrCreateDevWallet()
    }

    var isInitialized: Bool { sdk != nil }

    // MARK: - SDK lifecycle

    func initializeSDK() async {
        guard sdk == nil else { return }
        do {
            let storage = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("pollinet", isDirectory: true).path
            let config = SdkConfig(
                rpcUrl: rpcUrl.isEmpty ? nil : rpcUrl,
                enableLogging: true,
                logLevel: "info",
                storageDirectory: storage,
                encryptionKey: "pollinet-example-dev-key",
                walletAddress: walletAddress
            )
            let sdk = try await PolliNetSDK.initialize(config: config)
            self.sdk = sdk
            Self.sharedSDK = sdk

            let ble = BleController(sdk: sdk)
            ble.start()
            self.ble = ble

            if let wifiSdk = sdk.makeSharedWifiDirectSDK() {
                self.multipeer = MultipeerController(sdk: wifiSdk)
            }

            status = "SDK v\(PolliNetSDK.version()) · handle \(sdk.transportHandle) · \(sdk.transportKind())"
        } catch {
            lastError = error.localizedDescription
            status = "Init failed"
        }
    }

    func shutdown() {
        ble?.stop()
        multipeer?.stop()
        sdk?.shutdown()
        sdk = nil
        Self.sharedSDK = nil
        ble = nil
        multipeer = nil
        status = "Not initialized"
    }

    // MARK: - Wallet / approve flow

    private func loadOrCreateDevWallet() {
        let defaults = UserDefaults.standard
        if let raw = defaults.data(forKey: "devWalletKey"),
           let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: raw) {
            signingKey = key
        } else {
            let key = Curve25519.Signing.PrivateKey()
            defaults.set(key.rawRepresentation, forKey: "devWalletKey")
            signingKey = key
        }
        walletAddress = Base58.encode([UInt8](signingKey!.publicKey.rawRepresentation))
    }

    func refreshTokenAccounts() async {
        guard let sdk else { return }
        do {
            tokenAccounts = try await sdk.listTokenAccounts(walletAddress: walletAddress)
        } catch {
            lastError = error.localizedDescription
        }
    }

    /// Build (and submit, when online) the one-time delegate approval.
    /// Returns the Solana signature.
    func approve(tokenAccount: String, mint: String, amount: Int64, decimals: Int) async throws -> String {
        guard let sdk, let signingKey else {
            throw PolliNetError(code: "ERR_STATE", message: "SDK not initialized")
        }
        let blockhash = try await sdk.fetchRecentBlockhash()
        let response = try await sdk.createApproveTransaction(
            ownerWallet: walletAddress,
            tokens: [TokenApprovalEntry(
                mintAddress: mint, amount: amount, decimals: decimals, tokenAccount: tokenAccount
            )],
            recentBlockhash: blockhash
        )
        guard var txBytes = Data(base64Encoded: response.transaction) else {
            throw PolliNetError(code: "ERR_DECODE", message: "Bad approve tx encoding")
        }
        // Legacy Solana tx layout: [sig count][64B per sig][message]. The unsigned
        // tx carries one zeroed signature slot; sign the message and fill slot 0.
        let messageStart = 1 + 64
        guard txBytes.count > messageStart, txBytes.first == 1 else {
            throw PolliNetError(code: "ERR_SIGN", message: "Unexpected approve tx layout")
        }
        let signature = try signingKey.signature(for: txBytes[messageStart...])
        txBytes.replaceSubrange(1..<messageStart, with: signature)
        return try await sdk.submitSignedTransaction(signedTxBytes: txBytes)
    }

    // MARK: - Offline intent flow

    /// createIntentBytes → Ed25519 sign → intent-envelope JSON → mesh queue.
    /// Returns the queued txId.
    func sendIntent(toWallet: String, mint: String, amount: Int64, expiresInSeconds: Int64) async throws -> String {
        guard let sdk, let ble, let signingKey else {
            throw PolliNetError(code: "ERR_STATE", message: "SDK not initialized")
        }
        let toTokenAccount = try await sdk.deriveAssociatedTokenAccount(ownerWallet: toWallet, tokenMint: mint)
        let fromTokenAccount = try await sdk.deriveAssociatedTokenAccount(ownerWallet: walletAddress, tokenMint: mint)

        let intent = try await sdk.createIntentBytes(
            from: walletAddress,
            to: toTokenAccount,
            tokenMint: mint,
            amount: amount,
            expiresAt: Int64(Date().timeIntervalSince1970) + expiresInSeconds
        )
        guard let intentBytes = Data(base64Encoded: intent.intentBytes) else {
            throw PolliNetError(code: "ERR_DECODE", message: "Bad intent encoding")
        }
        let signature = try signingKey.signature(for: intentBytes)

        let envelope: [String: String] = [
            "intent_bytes": intent.intentBytes,
            "signature": signature.base64EncodedString(),
            "from_token_account": fromTokenAccount,
            "token_program": "spl-token",
        ]
        let envelopeData = try JSONSerialization.data(withJSONObject: envelope, options: [.sortedKeys])
        return try await ble.queueTransaction(base64: envelopeData.base64EncodedString())
    }
}
