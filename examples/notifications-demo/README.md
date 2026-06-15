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
  matrix-rust-sdk's `NotificationClient` would go.
- The handler then posts via `NotificationPlugin.postBackgroundNotification(...)`,
  reusing the plugin's channel/styling.

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
