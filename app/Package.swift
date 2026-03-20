// swift-tools-version: 6.0
import PackageDescription

// project.yml + XcodeGen are the canonical app build definition.
// This package manifest exists only for package-aware tooling around shared sources.

let package = Package(
    name: "Fawx",
    platforms: [
        .macOS(.v14),
        .iOS(.v17),
    ],
    products: [
        .executable(name: "Fawx", targets: ["Fawx"]),
    ],
    dependencies: [
        .package(url: "https://github.com/gonzalezreal/swift-markdown-ui.git", from: "2.4.1"),
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.5.0"),
    ],
    targets: [
        .executableTarget(
            name: "Fawx",
            dependencies: [
                .product(name: "MarkdownUI", package: "swift-markdown-ui"),
                .product(name: "Sparkle", package: "Sparkle", condition: .when(platforms: [.macOS])),
            ],
            path: "Fawx",
            exclude: [
                "Assets.xcassets",
                "Info.plist",
            ]
        ),
        .testTarget(
            name: "FawxTests",
            dependencies: ["Fawx"],
            path: "FawxTests",
            exclude: [
                "README.md",
            ]
        ),
    ]
)
