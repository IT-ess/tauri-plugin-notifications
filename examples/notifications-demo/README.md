# notifications-demo

Demo app for `tauri-plugin-notifications` (Tauri + SvelteKit). It exercises every
plugin feature; the notable one documented here is **silent push handling on
Android**, including delivery while the app is killed.

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

## Caveats

- `onMessageReceived` gives ~10–20s and Doze/background limits can delay or drop
  low-priority messages — send data messages with `"priority":"high"`, and if your
  real fetch may exceed the window, hand off to an expedited `WorkManager` job.
- `DebugSilentPushReceiver` is exported for adb testing only — remove it for
  production.
