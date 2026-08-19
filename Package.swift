// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Voice",
    platforms: [.macOS(.v13)],
    targets: [
        .target(
            name: "VoiceCore",
            path: "Sources/VoiceCore"
        ),
        .executableTarget(
            name: "Voice",
            dependencies: ["VoiceCore"],
            path: "Sources/VoiceApp"
        ),
        .executableTarget(
            name: "voice-cli",
            dependencies: ["VoiceCore"],
            path: "Sources/VoiceCLI"
        ),
        .testTarget(
            name: "VoiceCoreTests",
            dependencies: ["VoiceCore"],
            path: "Tests/VoiceCoreTests"
        ),
    ]
)
