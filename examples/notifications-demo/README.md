# notifications-demo

Demo app for `tauri-plugin-notifications` (Tauri + SvelteKit). It exercises every
plugin feature; the notable one documented here is **silent push handling on
Android and iOS**, including delivery while the app is killed (Android: FCM
service + JNI; iOS: Notification Service Extension).

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

### Warm path — `on_silent_push` (app alive)

Registered in `src-tauri/src/lib.rs` via `app.notifications().on_silent_push(...)`.
It only fires while the Tauri runtime is up. The **"Simulate Silent Push (Matrix)"**
button in the UI feeds a fake payload through this exact handler, which fetches
(simulated) content and shows a notification. No Firebase backend needed.

### Killed-state path — Kotlin `SilentPushHandler` + JNI (app killed)

When the OS has killed the app, Firebase still cold-starts the process and runs the
plugin's messaging service — but without the Tauri runtime, so `on_silent_push`
can't fire. The demo handles this with:

- `gen/android/app/src/main/java/com/test/app/DemoSilentPushHandler.kt` — implements
  the plugin's `SilentPushHandler`, declared on the plugin's FCM service via
  `<meta-data>` in `AndroidManifest.xml`. Runs in every state, killed included.
- `SilentPushBridge.kt` — loads the app's existing `.so` (which does **not** start
  Tauri) and calls a custom JNI entry.
- `src-tauri/src/android_push.rs` — that JNI entry; runs the (simulated) fetch on a
  short Tokio runtime and returns the notification content. This is the seam where
  matrix-rust-sdk's `NotificationClient` would go. The handler also receives the app
  **data directory** path (the same location Tauri's path API resolves to) and passes
  it through, so the fetch can open the same on-disk store the main app uses.
  It returns a chat-style payload (`conversationTitle`, `selfName`, and a `messages`
  array) including the sender **avatar as base64 bytes** — standing in for the bytes
  matrix-rust-sdk would return after downloading the `mxc://` avatar.
- The handler copies those fields onto the `Notification` and posts it; the plugin
  renders an Android **`MessagingStyle`** notification — circular avatar, sender
  name, room title, and an expandable long message — and decodes the base64 avatar.
- The handler then posts via `NotificationPlugin.postBackgroundNotification(...)`,
  reusing the plugin's channel/styling.
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
  - Warm path: `.deep_link(matrix_uri(&room_id, &event_id))` on the Rust builder.
  - Killed path: a `deepLink` field in the JNI handler's JSON, copied onto the
    `Notification`.
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

### Verify killed-state delivery with adb (no Firebase needed)

`DebugSilentPushReceiver` is a debug-only `BroadcastReceiver` that synthesizes a
silent push and runs the exact handler path above. Because it's manifest-registered,
an adb broadcast with `FLAG_INCLUDE_STOPPED_PACKAGES` cold-starts the **killed** app
into that path.

```bash
# Build + install the debug APK (x86_64 emulator shown)
pnpm tauri android build --apk --debug --target x86_64
adb install -r src-tauri/gen/android/app/build/outputs/apk/x86_64/debug/app-x86_64-debug.apk

# Launch once and grant the notification permission, then kill the app:
adb shell am force-stop com.alexis.notiftestapp

# Cold-start the killed app straight into the background silent-push path:
adb shell am broadcast -a com.alexis.notiftestapp.DEBUG_SILENT_PUSH -f 0x01000020 \
  --es room_id '!demo:matrix.org' --es event_id "evt$(date +%s)" \
  -n com.alexis.notiftestapp/.DebugSilentPushReceiver
```

Send it **twice for the same `room_id`** (different `event_id`s) and the two messages
stack into one conversation notification rather than posting separately:

```bash
for n in 1 2; do
  adb shell am broadcast -a com.alexis.notiftestapp.DEBUG_SILENT_PUSH -f 0x01000020 \
    --es room_id '!demo:matrix.org' --es event_id "evt$n-$(date +%s)" \
    -n com.alexis.notiftestapp/.DebugSilentPushReceiver
  sleep 1
done
```

`-f 0x01000020` = `FLAG_INCLUDE_STOPPED_PACKAGES` (`0x00000020`) +
`FLAG_RECEIVER_FOREGROUND` (`0x10000000`).

(Real Matrix event ids start with `$`; that's awkward to pass through `adb shell`
without the device shell trying to expand it, so this debug command uses a
`$`-free id. A real FCM payload has no such issue.)

Expected:
- A notification appears.
- `adb shell dumpsys activity activities | grep com.test.app` shows **no resumed
  activity** — the WebView/Activity was never started.
- `adb logcat | grep -E 'DemoSilentPushHandler|android_push|DebugSilentPush'` shows
  the JNI fetch and the background post.

### Real FCM (optional)

To test true FCM cold-start, add your own `google-services.json` under
`src-tauri/gen/android/app/`, register for push in the app to get the device token,
and send a **data-only** message via FCM HTTP v1 while the app is force-stopped:

```json
{ "message": { "token": "<device-token>",
  "data": { "room_id": "!r:hs", "event_id": "$abc" } } }
```

The same `DemoSilentPushHandler` runs.

## Silent push (iOS) — Notification Service Extension

iOS never restarts a force-quit app for a push, so the decode step runs in a
**Notification Service Extension** instead — a separate target
(`notifications-demo_NSE` in `src-tauri/gen/apple/project.yml`) that iOS
launches for every `mutable-content: 1` push, in any app state. Its principal
class subclasses the plugin's `TauriNotificationService`
(`gen/apple/NotificationService/NotificationService.swift`), which calls the
Rust handler `src-tauri/src/ios_push.rs` exports via
`ios_silent_push_handler!` from the same `libapp.a` the app links.

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
- `DebugSilentPushReceiver` is exported for adb testing only — remove it for
  production.
