// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "PolliNetSDK",
    platforms: [
        .iOS(.v15),
        .macOS(.v13), // host-side unit tests only; the SDK targets iOS
    ],
    products: [
        .library(name: "PolliNetSDK", targets: ["PolliNetSDK"])
    ],
    targets: [
        // Rust core (staticlib + cbindgen header), built by ../scripts/build_ios.sh
        .binaryTarget(name: "PolliNetRust", path: "PolliNetRust.xcframework"),
        .target(
            name: "PolliNetSDK",
            dependencies: ["PolliNetRust"]
        ),
        .testTarget(
            name: "PolliNetSDKTests",
            dependencies: ["PolliNetSDK"]
        ),
    ]
)
