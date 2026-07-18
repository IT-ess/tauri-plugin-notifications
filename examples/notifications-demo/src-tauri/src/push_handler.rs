//! The one silent-push handler for both mobile platforms.
//!
//! Registered with `silent_push_handler!`, which exports:
//! * the C entry point the iOS Notification Service Extension resolves via
//!   `dlsym` (the NSE links the same staticlib as the app; linking does not
//!   start Tauri), and
//! * the JNI entry point the plugin's Android FCM service calls after loading
//!   this library (named by the `SILENT_PUSH_LIB` manifest meta-data) — in
//!   every app state, including a killed-app cold start.
//!
//! There is no Tauri `AppHandle` in either context, so the handler returns a
//! [`NotificationData`] and the plugin renders it. In a real Matrix client,
//! [`crate::matrix_demo::simulate_matrix_fetch`] is where
//! `matrix_sdk::NotificationClient` would load and decrypt the event from the
//! store under `data_dir` — the App Group container on iOS, the app data
//! directory on Android.
//!
//! Declining (or failing in any way) never drops the push: iOS shows the
//! payload's own fallback `alert` (`loc-key: SINGLE_UNREAD`), Android falls
//! through to the JS `push-message` event.

use std::collections::HashMap;

use tauri_plugin_notifications::{NotificationData, NotificationMessage, SilentPushResponse};

use crate::matrix_demo;

/// Decode a Matrix push (`room_id` / `event_id` custom keys) into the
/// notification to display.
// The by-value map is the `SilentPushHandler` contract (the handler owns the data).
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn handle_silent_push(
    data_dir: &str,
    data: HashMap<String, String>,
) -> SilentPushResponse {
    // No room/event → not a Matrix message push; decline.
    let (Some(room_id), Some(event_id)) = (data.get("room_id").cloned(), data.get("event_id").cloned())
    else {
        return SilentPushResponse::Decline;
    };

    log::info!("silent push: fetching {event_id} in {room_id} (data dir: {data_dir})");

    // A real client opens its Matrix SDK store under `data_dir` here, e.g.
    // `{data_dir}/matrix/<user>/db`, then decrypts the event via
    // NotificationClient. We mirror that async fetch on a short-lived runtime.
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return SilentPushResponse::Decline;
    };
    let (sender, body) = runtime.block_on(async {
        // e.g. notification_client.get_notification(&room_id, &event_id).await
        matrix_demo::simulate_matrix_fetch(&room_id, &event_id)
    });

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));

    // One payload serves both platforms; each side ignores what it can't use:
    // * Android renders `messages` as a MessagingStyle conversation, keyed by
    //   `id` (stable per room, so events stack into one notification), and the
    //   tap opens `deep_link` via ACTION_VIEW → tauri-plugin-deep-link.
    // * iOS ignores `id` (APNs owns the request identifier) and `deep_link`;
    //   `group` (→ threadIdentifier) stacks the room, the last message becomes
    //   a communication notification, and the `extra` strings surface in the
    //   `notificationClicked` event's `data` on tap.
    SilentPushResponse::Notification(
        NotificationData::builder()
            .id(matrix_demo::notification_id_for(&room_id))
            .title(sender.as_str())
            .body(body.as_str())
            .group(room_id.as_str())
            .summary(room_id.as_str())
            .conversation_title(room_id.as_str())
            .group_conversation()
            .self_name("Me")
            .message(
                NotificationMessage::new(body)
                    .sender(sender.as_str())
                    .person_key(matrix_demo::sender_key(&sender))
                    .avatar_bytes(matrix_demo::DEMO_AVATAR.clone())
                    .timestamp(now_ms),
            )
            .deep_link(matrix_demo::matrix_uri(&room_id, &event_id))
            .extra(
                "deepLink",
                matrix_demo::matrix_uri(&room_id, &event_id),
            )
            .extra("room_id", room_id)
            .extra("event_id", event_id)
            .build(),
    )
}

tauri_plugin_notifications::silent_push_handler!(handle_silent_push);
