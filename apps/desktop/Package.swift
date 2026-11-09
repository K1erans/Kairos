// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "Kairos",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "KairosCore"),
        .executableTarget(name: "KairosHost", dependencies: ["KairosCore"]),
        .executableTarget(name: "Kairos", dependencies: ["KairosCore"]),
        .testTarget(name: "KairosCoreTests", dependencies: ["KairosCore"]),
    ]
)
