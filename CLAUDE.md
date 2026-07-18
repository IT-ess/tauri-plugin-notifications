# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Tauri v2 plugin for desktop + mobile notifications, published as two artifacts that must stay in sync:
- Rust crate `tauri-plugin-notifications` (crates.io) — the plugin host.
- npm package `@choochmeque/tauri-plugin-notifications-api` (`guest-js/` → built to `dist-js/`) — the JS/TS bindings apps call.

The JS layer is a thin wrapper that `invoke`s Rust commands; the Rust layer dispatches to a platform-specific backend. A change to the public API usually touches `guest-js/index.ts`, `src/commands.rs`, `src/models.rs`, the `COMMANDS` list in `build.rs`, `permissions/`, and every native backend.

## Commands

JS/TS (pnpm, package manager is pinned):
- `pnpm build` — rollup `guest-js/` → `dist-js/` (CJS + ESM + d.ts). `test`/`publish` depend on this.
- `pnpm test` / `pnpm test:watch` / `pnpm test:coverage` — vitest. Single test: `pnpm vitest run -t "<name>"` or `pnpm vitest run guest-js/index.test.ts`.
- `pnpm run format` / `format:check` — prettier (CI enforces `format:check`).

Rust:
- `cargo build` — desktop host. `cargo build --target aarch64-linux-android` / `aarch64-apple-ios` — generate mobile bindings (required before running native mobile tests).
- `cargo test --all-features` — Rust unit tests. Single: `cargo test <name>`. Windows-only suite runs as `cargo test --no-default-features -- windows::tests`.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features -- -D warnings -D clippy::unwrap_used` — CI gates. Clippy also runs per mobile target (`--target aarch64-linux-android` / `aarch64-apple-ios`). `cargo deny` and `cargo semver-checks` also gate PRs.
- `cargo llvm-cov --all-features --lcov --output-path coverage.lcov` — coverage as CI runs it.

Native test suites:
- Android: `cd android && ./gradlew testDebugUnitTest` (unit) or `connectedDebugAndroidTest` (instrumented, needs emulator). Sources in `android/src/main/java/app/tauri/notification/`.
- iOS/macOS: `cd ios && xcodebuild test -scheme tauri-plugin-notifications-Package -destination 'platform=iOS Simulator,name=iPhone 16,OS=latest'` (macOS uses `-scheme tauri-plugin-notifications -destination 'platform=macOS'`). The iOS scheme name differs because the iOS package has a second product (`tauri-plugin-notifications-nse`), so only the `-Package` scheme carries the test action.

Example app: `cd examples/notifications-demo && pnpm install && pnpm tauri dev`.

## Backend selection (the core of the architecture)

`src/lib.rs` `cfg`-gates which module provides `Notifications<R>` and re-exports it under one name. Exactly one backend compiles per target+feature combination, and they all expose the same method surface (`commands.rs` is written against that surface, target-agnostic):

- `src/desktop.rs` — `notify-rust` backend. Used on Linux always, and on macOS/Windows when the `notify-rust` feature is on (the default). On Linux it also tracks live `NotificationHandle`s in a map to implement `active`/`cancel`.
- `src/macos.rs` — native macOS backend, only when `not(feature = "notify-rust")`. Bridges to Swift via `swift-bridge`.
- `src/windows.rs` — native Windows toast backend, only when `not(feature = "notify-rust")`. Uses the `windows` crate + COM toast activator (CLSID from `WindowsConfig`).
- `src/mobile.rs` — iOS/Android. Calls into Kotlin/Swift through `run_mobile_plugin_async`.
- `src/unifiedpush.rs` — Linux push over D-Bus, only with `push-notifications`. Stateless/in-memory; the host app owns endpoint persistence via the `client_token`.

Feature flags (`Cargo.toml`): `notify-rust` (default desktop backend), `push-notifications` (off by default — enables FCM/APNs/UnifiedPush; pulls in zbus/tokio/uuid on Linux). To get native macOS/Windows backends, apps build with `default-features = false`.

## Cross-cutting things to keep in sync

- **Command registration:** every Tauri command must appear in the `COMMANDS` array in `build.rs` (drives codegen + permissions) AND have a matching entry under `permissions/`. Forgetting either breaks the build or makes the command unauthorized at runtime.
- **`build.rs` writes feature markers** the native build systems read: `android/build.properties` (`enablePushNotifications`) and `ios/.push-notifications-enabled` / `macos/.push-notifications-enabled` (Swift `Package.swift` toggles `ENABLE_PUSH_NOTIFICATIONS` from these).
- **`src/listeners.rs`** is a hand-rolled copy of Tauri's mobile plugin listener mechanism, because desktop plugins don't have one upstream yet. It backs notification-received / action / click events on desktop. Remove if/when Tauri adds desktop listener support.
- **`src/models.rs`** is the shared serde contract between JS and Rust (userInfo follows APNs convention — see recent history). JS `Options` in `guest-js/index.ts` must match it field-for-field (camelCase).
- **iOS and macOS Swift sources are near-duplicates** (`ios/Sources/` vs `macos/Sources/`) — changes to notification behavior usually need mirroring across both.
- **Silent-push (killed-state) contract:** `src/nse.rs` owns the host-handler entry points that `silent_push_handler!` exports — the C symbols `tauri_notifications_process_silent_push`/`_free` resolved via `dlsym` by `ios/NSE/Sources/TauriNotificationService.swift`, and the fixed JNI symbol `Java_app_tauri_notification_SilentPushNative_process` bound by `android/.../SilentPushNative.kt` (the messaging service loads the app lib named by the `app.tauri.notification.SILENT_PUSH_LIB` manifest meta-data). Renaming any of these breaks the other side silently; the JNI class/method is also pinned in `android/consumer-rules.pro`. The handler's returned `NotificationData` JSON is decoded by `ios/NSE/Sources/SilentPushContent.swift` (a hand-maintained copy of the serde contract) and by Jackson into the Kotlin `Notification` — new `NotificationData` fields need both updated.
- **Builder setters live once:** `notification_setters!` in `src/models.rs` generates the setter surface of both `NotificationDataBuilder` and `NotificationsBuilder` (`src/lib.rs`). Add new notification fields there, not in either impl; `const`-ness of existing setters is semver-frozen.

## Platform support gaps

Push notifications: iOS, Android, macOS, Linux only — **not Windows**. Linux push additionally requires a UnifiedPush *distributor* app (ntfy, NextPush, etc.) installed on the user's system.
