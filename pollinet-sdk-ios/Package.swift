// swift-tools-version: 5.9
import PackageDescription

// ─────────────────────────────────────────────────────────────────────────────
// Distribution mode
//
// The XCFramework (pre-built Rust static libs) is published as a GitHub
// Release asset rather than committed to the repo (the .a files exceed
// GitHub's 100 MB per-file limit).
//
// LOCAL DEVELOPMENT
//   1. Run ./scripts/build-xcframework.sh to build PolliNetRust.xcframework
//   2. Flip the `useLocalXCFramework` flag to true below.
//
// RELEASE CONSUMPTION (default)
//   SPM downloads the XCFramework zip from the GitHub Release URL.
//   Update `xcframeworkUrl` and `xcframeworkChecksum` when publishing a new
//   release (generate the checksum with: swift package compute-checksum <zip>)
// ─────────────────────────────────────────────────────────────────────────────

let useLocalXCFramework = false   // flip to true during local development

let xcframeworkUrl      = "https://github.com/pollinet/pollinet/releases/download/v0.1.4-ios/PolliNetRust.xcframework.zip"
let xcframeworkChecksum = "4f31127454fb0c6e47f67c71f5dd55408b5dd512f9dbae3af20d282d96071512"

// ─────────────────────────────────────────────────────────────────────────────

let rustTarget: Target = useLocalXCFramework
    ? .binaryTarget(
        name: "PolliNetRust",
        path: "PolliNetRust.xcframework"
    )
    : .binaryTarget(
        name: "PolliNetRust",
        url: xcframeworkUrl,
        checksum: xcframeworkChecksum
    )

let package = Package(
    name: "PolliNetSDK",
    platforms: [
        .iOS(.v16)
    ],
    products: [
        .library(
            name: "PolliNetSDK",
            targets: ["PolliNetSDK"]
        )
    ],
    targets: [
        rustTarget,

        // C bridging shim — exposes the header to Swift
        .target(
            name: "PolliNetFFI",
            dependencies: ["PolliNetRust"],
            path: "Sources/PolliNetFFI",
            sources: ["PolliNetFFI.c"],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("include")
            ]
        ),

        // Swift wrapper — the public API consumers use
        .target(
            name: "PolliNetSDK",
            dependencies: ["PolliNetFFI"],
            path: "Sources/PolliNetSDK",
            swiftSettings: [
                .enableExperimentalFeature("StrictConcurrency")
            ]
        )
    ]
)
