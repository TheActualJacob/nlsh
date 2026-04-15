// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "nlsh-model",
    platforms: [.macOS(.v26)],
    targets: [
        .executableTarget(
            name: "nlsh-model",
            path: "Sources/nlsh-model"
        )
    ]
)
