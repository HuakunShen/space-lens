// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "icloud-free",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "ICloudFreeCore", targets: ["ICloudFreeCore"]),
        .executable(name: "icloud-free", targets: ["icloud-free"]),
        .executable(name: "ICloudFreeApp", targets: ["ICloudFreeApp"]),
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser.git", from: "1.5.0"),
    ],
    targets: [
        .target(name: "ICloudFreeCore"),
        .executableTarget(
            name: "icloud-free",
            dependencies: [
                "ICloudFreeCLI",
            ]
        ),
        .target(
            name: "ICloudFreeCLI",
            dependencies: [
                "ICloudFreeCore",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ]
        ),
        .target(name: "ICloudFreeAppSupport", dependencies: ["ICloudFreeCore"]),
        .executableTarget(
            name: "ICloudFreeApp",
            dependencies: ["ICloudFreeCore", "ICloudFreeAppSupport"],
            resources: [.process("Resources")]
        ),
        .testTarget(
            name: "ICloudFreeCoreTests",
            dependencies: ["ICloudFreeCore", "ICloudFreeCLI", "ICloudFreeAppSupport"]
        ),
    ]
)
