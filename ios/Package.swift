// swift-tools-version:5.7

import PackageDescription
import Foundation

// Check if push notifications feature is enabled via marker file from Rust build
let enablePushNotifications = FileManager.default.fileExists(
  atPath: URL(fileURLWithPath: #file).deletingLastPathComponent()
    .appendingPathComponent(".push-notifications-enabled").path
)

var swiftSettings: [SwiftSetting] = []
if enablePushNotifications {
  swiftSettings.append(.define("ENABLE_PUSH_NOTIFICATIONS"))
}

let package = Package(
  name: "tauri-plugin-notifications",
  platforms: [
    .macOS(.v13),
    .iOS(.v15),
  ],
  products: [
    // Products define the executables and libraries a package produces, and make them visible to other packages.
    .library(
      name: "tauri-plugin-notifications",
      type: .static,
      targets: ["tauri-plugin-notifications"]),
    // Tauri-free support library for the host app's Notification Service
    // Extension target. Only this product may be linked into an NSE — the main
    // plugin product depends on Tauri, which must not run in an extension.
    .library(
      name: "tauri-plugin-notifications-nse",
      type: .static,
      targets: ["TauriPluginNotificationsNSE"]),
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    // Targets are the basic building blocks of a package. A target can define a module or a test suite.
    // Targets can depend on other targets in this package, and on products in packages this package depends on.
    .target(
      name: "tauri-plugin-notifications",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources",
      swiftSettings: swiftSettings),
    // No Tauri dependency: this module runs inside the NSE process. The target
    // name (not the dashed product name) is the module hosts `import`.
    .target(
      name: "TauriPluginNotificationsNSE",
      path: "NSE/Sources"),
    .testTarget(
        name: "PluginTests",
        dependencies: ["tauri-plugin-notifications", .byName(name: "Tauri")]
    ),
    .testTarget(
        name: "NSETests",
        dependencies: ["TauriPluginNotificationsNSE"],
        path: "NSE/Tests"
    ),
  ]
)
