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
        // Command-line snippet editor bundled into Voice.app by build.sh.
        // Named voicectl rather than voice: the app binary is `Voice`, and a
        // `voice` product would collide with it on a case-insensitive disk.
        .executableTarget(
            name: "voicectl",
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
