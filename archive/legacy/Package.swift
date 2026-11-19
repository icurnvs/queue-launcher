// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "QueueLauncher",
    platforms: [.macOS(.v14)],
    targets: [.executableTarget(name: "QueueLauncher")]
)
