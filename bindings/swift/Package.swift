// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Synx",
    products: [
        .library(name: "Synx", targets: ["Synx"]),
    ],
    targets: [
        .systemLibrary(name: "CSynx", path: "Sources/CSynx"),
        .target(
            name: "Synx",
            dependencies: ["CSynx"]
        ),
        .testTarget(
            name: "SynxTests",
            dependencies: ["Synx"]
        ),
    ]
)
