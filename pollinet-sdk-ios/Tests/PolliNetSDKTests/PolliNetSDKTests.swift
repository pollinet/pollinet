//
//  PolliNetSDKTests.swift
//  Engine-level tests over the real Rust core (via the macOS slice of
//  PolliNetRust.xcframework) — the Swift counterpart of the checks the
//  Android SDK relies on.
//
//  NOTE: per the executor's ring-buffer semantics an all-zero nonce is always
//  rejected on-chain — every nonce used here is non-zero.
//

import CryptoKit
import XCTest
@testable import PolliNetSDK

final class PolliNetSDKTests: XCTestCase {

    private func makeSDK() async throws -> PolliNetSDK {
        try await PolliNetSDK.initialize(config: SdkConfig(enableLogging: false, logLevel: nil))
    }

    // MARK: - Lifecycle

    func testVersionIsNonEmpty() {
        XCTAssertFalse(PolliNetSDK.version().isEmpty)
    }

    func testInitializeAndShutdown() async throws {
        let sdk = try await makeSDK()
        XCTAssertGreaterThanOrEqual(sdk.transportHandle, 0)
        XCTAssertEqual(sdk.transportKind(), "BLE")
        sdk.shutdown()
        // After shutdown the handle must be rejected.
        do {
            _ = try await sdk.metrics()
            XCTFail("metrics() should fail after shutdown")
        } catch let error as PolliNetError {
            XCTAssertEqual(error.code, "ERR_INTERNAL")
        }
    }

    func testSharedWifiDirectHandle() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let shared = sdk.createSharedWifiDirectHandle()
        XCTAssertGreaterThanOrEqual(shared, 0)
        XCTAssertNotEqual(shared, sdk.transportHandle)
    }

    // MARK: - Transport pump basics

    func testMetricsAndTick() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let metrics = try await sdk.metrics()
        XCTAssertEqual(metrics.fragmentsBuffered, 0)
        let frames = try await sdk.tick()
        XCTAssertTrue(frames.isEmpty)
        let idle = await sdk.nextOutbound(maxLen: 244)
        XCTAssertNil(idle)
    }

    // MARK: - Fragmentation round-trip

    func testFragmentReconstructRoundTrip() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }

        let original = Data((0..<1000).map { _ in UInt8.random(in: 0...255) })
        let list = try await sdk.fragment(txBytes: original, maxPayload: 200)
        XCTAssertGreaterThan(list.fragments.count, 1)

        // Fragment.id is a short display id; the wire tx id is SHA-256 of the
        // transaction bytes, and reconstruction verifies it.
        let txId = SHA256.hash(data: original).map { String(format: "%02x", $0) }.joined()
        let inputs = list.fragments.map {
            FragmentData(
                transactionId: txId,
                fragmentIndex: $0.index,
                totalFragments: $0.total,
                dataBase64: $0.data
            )
        }
        let reconstructedBase64 = try await sdk.reconstructTransaction(fragments: inputs)
        XCTAssertEqual(Data(base64Encoded: reconstructedBase64), original)
    }

    func testFragmentationStats() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let stats = try await sdk.getFragmentationStats(transactionBytes: Data(repeating: 7, count: 800))
        XCTAssertEqual(stats.originalSize, 800)
        XCTAssertGreaterThan(stats.fragmentCount, 0)
    }

    // MARK: - Outbound queue: push/pop + priority ordering

    func testOutboundQueuePriorityOrdering() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }

        func push(_ txId: String, _ priority: Priority) async throws {
            let payload = Data(repeating: 1, count: 64)
            let fragment = FragmentFFI(
                transactionId: txId,
                fragmentIndex: 0,
                totalFragments: 1,
                dataBase64: payload.base64EncodedString()
            )
            try await sdk.pushOutboundTransaction(
                txBytes: payload, txId: txId, fragments: [fragment], priority: priority
            )
        }

        let lowId = String(repeating: "a", count: 64)
        let highId = String(repeating: "b", count: 64)
        try await push(lowId, .low)
        try await push(highId, .high)

        let queueSize = try await sdk.getOutboundQueueSize()
        XCTAssertGreaterThanOrEqual(queueSize, 0)

        let first = try await sdk.popOutboundTransaction()
        XCTAssertEqual(first?.txId, highId, "HIGH priority must pop before LOW")
        let second = try await sdk.popOutboundTransaction()
        XCTAssertEqual(second?.txId, lowId)
        let empty = try await sdk.popOutboundTransaction()
        XCTAssertNil(empty)
    }

    // MARK: - Retry queue

    func testRetryQueuePushAndSize() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        try await sdk.addToRetryQueue(
            txBytes: Data(repeating: 9, count: 32),
            txId: String(repeating: "c", count: 64),
            error: "rpc timeout"
        )
        let size = try await sdk.getRetryQueueSize()
        XCTAssertEqual(size, 1)
        // A fresh retry item is ready immediately (backoff starts after the first attempt).
        let ready = try await sdk.popReadyRetry()
        XCTAssertEqual(ready?.txId, String(repeating: "c", count: 64))
        XCTAssertEqual(ready?.lastError, "rpc timeout")
        let drained = try await sdk.getRetryQueueSize()
        XCTAssertEqual(drained, 0)
    }

    // MARK: - Confirmations

    func testConfirmationQueueRoundTrip() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let txId = String(repeating: "d", count: 64)
        try await sdk.queueConfirmation(txId: txId, signature: "sig-abc")
        let popped = try await sdk.popConfirmation()
        XCTAssertEqual(popped?.txId, txId)
        XCTAssertEqual(popped?.status, .success(signature: "sig-abc"))
        let empty = try await sdk.popConfirmation()
        XCTAssertNil(empty)
    }

    func testQueueFailureConfirmation() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let txId = String(repeating: "e", count: 64)
        try await sdk.queueFailureConfirmation(txId: txId, error: "expired")
        let popped = try await sdk.popConfirmation()
        XCTAssertEqual(popped?.txId, txId)
        if case .failed(let error)? = popped?.status {
            XCTAssertEqual(error, "expired")
        } else {
            XCTFail("Expected FAILED status, got \(String(describing: popped?.status))")
        }
    }

    // MARK: - Received queue

    func testReceivedQueuePushAndDedup() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let tx = Data((0..<200).map { _ in UInt8.random(in: 0...255) })

        let first = try await sdk.pushReceivedTransaction(transactionBytes: tx)
        XCTAssertTrue(first.added)
        XCTAssertEqual(first.queueSize, 1)

        // Same bytes again — deduplicated.
        let second = try await sdk.pushReceivedTransaction(transactionBytes: tx)
        XCTAssertFalse(second.added)

        let received = try await sdk.nextReceivedTransaction()
        XCTAssertEqual(received.flatMap { Data(base64Encoded: $0.transactionBase64) }, tx)
        let empty = try await sdk.nextReceivedTransaction()
        XCTAssertNil(empty)
    }

    // MARK: - Intent: 169 bytes + nonce + offline fee fallback

    func testCreateIntentBytesIs169BytesWithSuppliedNonce() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }

        // Non-zero nonce — the executor's zeroed ring buffer rejects all-zero nonces.
        let nonceHex = "0102030405060708090a0b0c0d0e0f10"
        let response = try await sdk.createIntentBytes(
            from: "11111111111111111111111111111111",
            to: "So11111111111111111111111111111111111111112",
            tokenMint: "So11111111111111111111111111111111111111112",
            amount: 1_000_000,
            expiresAt: Int64(Date().timeIntervalSince1970) + 3600,
            nonceHex: nonceHex
        )

        let bytes = Data(base64Encoded: response.intentBytes)
        XCTAssertEqual(bytes?.count, 169, "canonical intent layout is exactly 169 bytes")
        XCTAssertEqual(response.nonceHex, nonceHex)
        XCTAssertEqual(bytes?.first, 1, "intent version byte must be 1")
    }

    func testCreateIntentBytesRandomNonceIsNonZero() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let response = try await sdk.createIntentBytes(
            from: "11111111111111111111111111111111",
            to: "So11111111111111111111111111111111111111112",
            tokenMint: "So11111111111111111111111111111111111111112",
            amount: 500,
            expiresAt: Int64(Date().timeIntervalSince1970) + 600
        )
        XCTAssertEqual(response.nonceHex.count, 32)
        XCTAssertNotEqual(response.nonceHex, String(repeating: "0", count: 32))
    }

    func testDeriveAssociatedTokenAccountIsDeterministic() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let a = try await sdk.deriveAssociatedTokenAccount(
            ownerWallet: "11111111111111111111111111111111",
            tokenMint: "So11111111111111111111111111111111111111112"
        )
        let b = try await sdk.deriveAssociatedTokenAccount(
            ownerWallet: "11111111111111111111111111111111",
            tokenMint: "So11111111111111111111111111111111111111112"
        )
        XCTAssertEqual(a, b)
        XCTAssertFalse(a.isEmpty)
    }

    func testGetExecutorPda() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let pda = try await sdk.getExecutorPda()
        XCTAssertFalse(pda.pda.isEmpty)
    }

    // MARK: - Tombstones / maintenance

    func testTombstonesStartEmpty() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let count = try await sdk.getTombstoneCount()
        XCTAssertEqual(count, 0)
        let tombstoned = try await sdk.isTombstoned(txIdHashHex: String(repeating: "f", count: 64))
        XCTAssertFalse(tombstoned)
        try await sdk.periodicMaintenance()
    }

    // MARK: - Density subsystem

    func testDensityAndCooldowns() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        try await sdk.recordScanResult(peerId: "AA:BB:CC:DD:EE:01")
        try await sdk.recordScanResult(peerId: "AA:BB:CC:DD:EE:02")
        let params = try await sdk.getAdaptiveParams()
        XCTAssertGreaterThan(params.sessionTargetMs, 0)

        try await sdk.addPeerToCooldown(peerId: "AA:BB:CC:DD:EE:01", cooldownMs: 60_000)
        let cooling = try await sdk.isPeerInCooldown(peerId: "AA:BB:CC:DD:EE:01")
        XCTAssertTrue(cooling)
        let released = try await sdk.expireOldestCooldown()
        XCTAssertEqual(released, "AA:BB:CC:DD:EE:01")
    }

    // MARK: - Wallet address

    func testWalletAddressRoundTrip() async throws {
        let sdk = try await makeSDK()
        defer { sdk.shutdown() }
        let before = try await sdk.getWalletAddress()
        XCTAssertNil(before)
        try await sdk.setWalletAddress("11111111111111111111111111111111")
        let after = try await sdk.getWalletAddress()
        XCTAssertEqual(after, "11111111111111111111111111111111")
        try await sdk.setWalletAddress(nil)
        let cleared = try await sdk.getWalletAddress()
        XCTAssertNil(cleared)
    }
}
