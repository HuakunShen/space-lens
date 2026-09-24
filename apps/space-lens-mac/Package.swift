// swift-tools-version: 6.0

import PackageDescription
import Foundation

let ffiLibraryDirectory = ProcessInfo.processInfo.environment["SPACE_LENS_FFI_LIB_DIR"] ?? "../../target/debug"

let package = Package(
    name: "space-lens-mac",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "SpaceLensApp", targets: ["SpaceLensMacApp"]),
    ],
    targets: [
        .target(name: "SpaceLensFFIBridge", publicHeadersPath: "include"),
        .executableTarget(
            name: "SpaceLensMacApp",
            dependencies: ["SpaceLensFFIBridge"],
            resources: [.process("Resources")],
            linkerSettings: [
                .unsafeFlags(["-L\(ffiLibraryDirectory)", "-lspace_lens_ffi"]),
            ]
        ),
        .testTarget(
            name: "SpaceLensMacAppTests",
            dependencies: ["SpaceLensMacApp"]
        ),
    ]
)
