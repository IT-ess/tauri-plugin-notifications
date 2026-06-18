// Android-only background (killed-state) silent-push handling: a JNI entry the
// FCM service calls when there is no Tauri runtime. See the module docs.
#[cfg(target_os = "android")]
mod android_push;

/// Simulates handling a *silent* (data-only) push for a Matrix-style client on
/// the **warm** path — i.e. while the app/Tauri runtime is alive, driven by
/// `on_silent_push`. Here we can use the plugin builder directly.
///
/// The **killed** path can't use the builder (no `AppHandle`); it goes through
/// `android_push`'s JNI entry instead, but shares the same `simulate_matrix_fetch`.
#[cfg(target_os = "android")]
fn process_silent_push<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data: &std::collections::HashMap<String, String>,
) {
    use tauri_plugin_notifications::{NotificationMessage, NotificationsExt};

    let room_id = data
        .get("room_id")
        .cloned()
        .unwrap_or_else(|| "!unknown:matrix.org".to_string());
    let event_id = data
        .get("event_id")
        .cloned()
        .unwrap_or_else(|| "$unknown".to_string());

    log::info!("silent push (warm): fetching event {event_id} in room {room_id}");

    // Stand-in for `GET /_matrix/client/v3/rooms/{room_id}/event/{event_id}`.
    let (sender, body) = android_push::simulate_matrix_fetch(&room_id, &event_id);
    // Key the notification by the room so repeated events accumulate into one
    // MessagingStyle conversation (tap the demo button twice to see it stack).
    let id = android_push::notification_id_for(&room_id);
    let sender_key = format!("@{}:matrix.org", sender.to_lowercase());

    let builder = app
        .notifications()
        .builder()
        .id(id)
        .conversation_title(room_id.as_str())
        .group_conversation()
        .self_name("Me")
        .message(
            NotificationMessage::new(body)
                .sender(sender)
                .person_key(sender_key)
                .avatar_bytes(android_push::demo_avatar_base64()),
        )
        .extra("room_id", room_id)
        .extra("event_id", event_id);

    // `show()` is async on mobile; the silent-push handler runs on a background
    // thread, so spawn the display work rather than blocking it.
    tauri::async_runtime::spawn(async move {
        if let Err(e) = builder.show().await {
            log::error!("failed to show notification from silent push: {e}");
        }
    });
}

/// Demo-only command: feed a fake silent push through the same handler the FCM
/// data message would, so the flow is testable without a Firebase backend. In
/// production the identical `process_silent_push` runs from `on_silent_push`.
// Tauri command handlers take owned args by convention (see the plugin's own
// `commands.rs`), and these are only consumed on Android.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
#[cfg_attr(not(target_os = "android"), allow(unused_variables))]
fn simulate_silent_push(app: tauri::AppHandle, room_id: String, event_id: String) {
    #[cfg(target_os = "android")]
    {
        let mut data = std::collections::HashMap::new();
        data.insert("room_id".to_string(), room_id);
        data.insert("event_id".to_string(), event_id);
        process_silent_push(&app, &data);
    }
    #[cfg(not(target_os = "android"))]
    log::warn!("simulate_silent_push is Android-only");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Show INFO and above from everything by default, plus DEBUG from the
    // notifications plugin so UnifiedPush / D-Bus events are visible while
    // exercising the demo. Override at runtime with RUST_LOG=...
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("info,tauri_plugin_notifications=debug"),
    )
    .format_timestamp_millis()
    .init();

    log::info!("notifications-demo starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notifications::init())
        .invoke_handler(tauri::generate_handler![simulate_silent_push])
        .setup(|app| {
            // Register the Rust-only silent-push handler. On Android, data-only
            // FCM messages are routed here so we can fetch content and raise the
            // notification ourselves — the Matrix client pattern.
            #[cfg(target_os = "android")]
            {
                use tauri_plugin_notifications::NotificationsExt;
                let handle = app.handle().clone();
                if let Err(e) = app.notifications().on_silent_push(move |push| {
                    log::info!("silent push received: {:?}", push.data);
                    process_silent_push(&handle, &push.data);
                }) {
                    log::error!("failed to register silent push handler: {e}");
                }
            }
            #[cfg(not(target_os = "android"))]
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
