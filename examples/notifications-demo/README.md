# notifications-demo

Demo app for `tauri-plugin-notifications` (Tauri + SvelteKit). It exercises every
plugin feature; the notable one documented here is **silent push handling on
Android and iOS**, including delivery while the app is killed — one Rust handler
(`src-tauri/src/push_handler.rs`, registered with `silent_push_handler!`) serves
both platforms (Android: the plugin's FCM service; iOS: Notification Service
Extension).

## Run (desktop)

```bash
pnpm install
pnpm tauri dev
```

## Silent push (Android)

A *silent* push is a data-only FCM message (no `notification` block): Android shows
nothing and hands the app just the data payload (e.g. a Matrix `room_id` /
`event_id`). The app is expected to fetch the real content and raise the
notification itself. This demo shows both delivery paths.

### One handler, every app state

The plugin's FCM service handles data-only messages itself, in every app state:
it loads the app's native library (named by the
`app.tauri.notification.SILENT_PUSH_LIB` meta-data in `AndroidManifest.xml`,
which does **not** start Tauri), calls the Rust handler registered with
`silent_push_handler!`, and posts the returned notification. The demo pieces:

- `src-tauri/src/push_handler.rs` — the handler; runs the (simulated) fetch on a
  short Tokio runtime and returns the notification content. This is the seam where
  matrix-rust-sdk's `NotificationClient` would go. It receives the app
  **data directory** path (the same location Tauri's path API resolves to), so the
  fetch can open the same on-disk store the main app uses. It returns a chat-style
  payload (`conversation_title`, `self_name`, and `messages`) including the sender
  **avatar as base64 bytes** — standing in for the bytes matrix-rust-sdk would
  return after downloading the `mxc://` avatar.
- The plugin renders an Android **`MessagingStyle`** notification — circular
  avatar, sender name, room title, and an expandable long message — via the same
  channel/styling as foreground notifications.
- The **"Simulate Silent Push (Matrix)"** button in the UI feeds a fake payload
  through this exact handler and shows the result — no Firebase backend needed
  (warm only; for killed-state delivery see below).
- The notification **id is keyed by the room** (`notification_id_for(&room_id)`) and
  `appendMessages` is on, so multiple events in the same room **stack** into one
  conversation notification — the plugin appends each new message to the one already
  showing, even across cold starts.
- **Tapping the notification opens the room via a `matrix:` deep link.** The payload
  sets the notification's **`deepLink`** to a Matrix URI (MSC2312), e.g.
  `matrix:roomid/abc:matrix.org/e/xyz`. The plugin makes the tap fire an `ACTION_VIEW`
  intent for that URI **targeted at the app's own launcher activity by explicit
  component**, so the OS routes it straight there — never a chooser, even though
  another `matrix:` client may be installed (on this repo's test emulator both
  `com.alexis.notiftestapp` *and* a real `com.matrix.svelte.client` register the
  scheme; an unpinned intent would pop the system chooser). [`tauri-plugin-deep-link`]
  then reports the URL to JS. The demo's `onOpenUrl` (and `getCurrent()` for the
  cold-start case, where the deep link *launched* the app) parses the URI and opens
  the room — the banner at the top of the page. This is the same entry point any other
  `matrix:` link would hit, so notifications, links, and `matrix.to` redirects all
  converge on one handler.
  - Both paths set it the same way: `.deep_link(matrix_uri(&room_id, &event_id))`
    in `push_handler.rs`.
  - Because the tap is now `ACTION_VIEW`, it **replaces** the plugin's
    `notificationClicked` event for these notifications (that event still fires for
    notifications without a `deepLink`).
  - The custom `matrix:` scheme filter is declared manually in `AndroidManifest.xml`
    (the deep-link plugin only auto-generates app-link filters for verified https
    hosts). It's still needed for *external* `matrix:` links; the notification tap
    itself bypasses it via the explicit component.
  - **Reliability note:** the tap intent carries a dummy MIME type
    (`application/octet-stream`). This is a workaround for a crash in `tao` (Tauri's
    windowing layer): an `ACTION_VIEW` intent whose `getType()` is `null` panics tao's
    intent handler, and the panic aborts the process. A non-null type sidesteps it;
    tao still reads the deep link from the data URI. The plugin applies this
    automatically — without it, every custom-scheme deep-link tap would crash the app.

[`tauri-plugin-deep-link`]: https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/deep-link

Everything runs in the **main process** — no separate `android:process` is required.

### Verify killed-state delivery (real FCM)

Killed-state delivery needs a real FCM data message (a cold start driven by
Firebase itself). Add your own `google-services.json` under
`src-tauri/gen/android/app/` (a placeholder is committed so builds work without
one), register for push in the app to get the device token, force-stop the app,
and send a **data-only** message:

```bash
# Build + install the debug APK (x86_64 emulator shown)
pnpm tauri android build --apk --debug --target x86_64
adb install -r src-tauri/gen/android/app/build/outputs/apk/x86_64/debug/app-x86_64-debug.apk

# Launch once, grant the notification permission, tap "Register" to log the
# device token, then kill the app:
adb shell am force-stop com.alexis.notiftestapp

# Send a data-only push via FCM HTTP v1 (needs a service-account.json):
./scripts/send-silent-push.sh <device-token> '!demo:matrix.org'
```

Send it **twice for the same `room_id`** (different `event_id`s) and the two
messages stack into one conversation notification rather than posting separately.

Expected:
- A notification appears.
- `adb shell dumpsys activity activities | grep com.alexis.notiftestapp` shows
  **no resumed activity** — the WebView/Activity was never started.
- `adb logcat | grep NotificationsPlugin` shows the FCM arrival, the native
  fetch, and the background post.

## Silent push (iOS) — Notification Service Extension

iOS never restarts a force-quit app for a push, so the decode step runs in a
**Notification Service Extension** instead — a separate target
(`notifications-demo_NSE` in `src-tauri/gen/apple/project.yml`) that iOS
launches for every `mutable-content: 1` push, in any app state. Its principal
class subclasses the plugin's `TauriNotificationService`
(`gen/apple/NotificationService/NotificationService.swift`), which calls the
same Rust handler `src-tauri/src/push_handler.rs` exports via
`silent_push_handler!` from the same `libapp.a` the app links.

### Build it, and what the Simulator can(not) show

```sh
# Build + run on a simulator (also builds and embeds the NSE):
pnpm tauri ios dev "iPhone 17"

# Grant the notification permission in the app, then push the Matrix payload:
xcrun simctl push booted com.alexis.notiftestapp scripts/matrix-push.apns

# Confirm the extension is installed and registered with the system:
xcrun simctl spawn booted pluginkit -m -i com.alexis.notiftestapp.nse
```

**Important Simulator limitation:** payloads injected with `xcrun simctl push`
are *not* routed through Notification Service Extensions — the banner shows the
payload's own fallback alert (`SINGLE_UNREAD`), which proves delivery, permission,
and the fallback path, but **not** the rewrite. To watch the NSE actually rewrite
the push you need a **real APNs push**:

- a physical device, or
- an Apple-Silicon Mac Simulator, which accepts real **APNs sandbox** pushes
  (Xcode 14+): register for push in the app to obtain the simulator's device
  token, then send the payload to `api.sandbox.push.apple.com` with your `.p8`
  key for the app's bundle id.

Then expect:
- A banner shows **Alice** and the fetched message body — the NSE rewrote the
  push, whose own alert is just the `SINGLE_UNREAD` fallback.
- It also works with the app **force-quit** (swipe it away in the app switcher
  first) — the whole point over `content-available` background pushes.
- Send one without `room_id`: the handler declines (`None`) and the untouched
  fallback alert shows instead.
- Tap the banner with the app running: the demo UI logs `notificationClicked`
  with `data.deepLink` / `room_id` / `event_id` (`id` is `-1` for remote
  notifications — the NSE can't change the identifier APNs assigned).
- NSE logs: Console.app → simulator/device → filter subsystem
  `app.tauri.notifications`.

The extension flow itself (payload flattening → `dlsym` into Rust → JSON decode
→ content rewrite, plus every fallback path) is covered end-to-end by the
plugin's Swift tests in `ios/NSE/Tests`, which stub the Rust symbols from the
test bundle — so it runs in CI without APNs.

The app group (`group.com.alexis.notiftestapp`, on both targets) is what a real Matrix
client uses to share its store: the extension passes the group container path
to the Rust handler as `data_dir`.

If Xcode's package resolution can't find the plugin's Swift package, run
`cargo build --target aarch64-apple-ios` once at the repo root (it generates
`.tauri/tauri-api`, which `ios/Package.swift` depends on), and after editing
`project.yml` re-run `xcodegen generate` in `src-tauri/gen/apple`.

## Caveats

- `onMessageReceived` gives ~10–20s and Doze/background limits can delay or drop
  low-priority messages — send data messages with `"priority":"high"`, and if your
  real fetch may exceed the window, hand off to an expedited `WorkManager` job.
