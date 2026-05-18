// swift-tools-version: 5.9
//
// SYNX — native Swift parser. Parity with crates/synx-core 3.6.x.
//
// Targets: macOS 12+, iOS 15+, watchOS 8+, tvOS 15+, Linux (5.9+ toolchain).
// No FFI to synx-c; the entire engine is pure Swift.
import PackageDescription

let package = Package(
    name: "Synx",
    platforms: [
        .macOS(.v12),
        .iOS(.v15),
        .tvOS(.v15),
        .watchOS(.v8),
    ],
    products: [
        .library(name: "Synx", targets: ["Synx"]),
    ],
    targets: [
        .target(
            name: "Synx",
            path: "Sources/Synx"
        ),
        .testTarget(
            name: "SynxTests",
            dependencies: ["Synx"],
            path: "Tests/SynxTests",
            resources: [
                // Conformance corpus is referenced from disk relative to the
                // workspace; tests skip silently when it's not bundled.
            ]
        ),
    ]
)
