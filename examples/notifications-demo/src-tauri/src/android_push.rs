//! Android-only background silent-push handling.
//!
//! When the app is killed, Firebase cold-starts the process and runs the
//! notifications plugin's messaging service *without* the Tauri runtime. The
//! plugin's `SilentPushHandler` (see `DemoSilentPushHandler.kt`) then calls the
//! JNI entry point below to "fetch" the notification content. There is no Tauri
//! `AppHandle` in this state, so we cannot use the plugin builder — we just
//! return the content as JSON and let Kotlin post the notification.
//!
//! In a real Matrix client, [`simulate_matrix_fetch`] is where
//! `matrix_sdk::NotificationClient` would load and decrypt the event from the
//! on-disk store the main app shares.

// The shared helpers are `pub(crate)` for use from `lib.rs`; this module is
// private, so clippy flags that as redundant — it isn't, the parent needs them.
#![allow(clippy::redundant_pub_crate)]

use std::collections::HashMap;

use base64::Engine;
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;

use crate::matrix_demo::{matrix_uri, simulate_matrix_fetch};

/// Base64-encoded demo avatar. Stands in for the bytes a real client gets from
/// matrix-sdk's media store after downloading the sender/room `mxc://` avatar;
/// here we just reuse the app icon so no extra asset is committed.
pub(crate) fn demo_avatar_base64() -> String {
    const AVATAR_PNG: &[u8] = include_bytes!("../icons/testavatar.png");
    base64::engine::general_purpose::STANDARD.encode(AVATAR_PNG)
}

/// Derive a stable, positive notification id from a conversation key (the room
/// id). Using the room as the key means every message in that room lands in the
/// same notification, so the plugin accumulates them into one conversation
/// instead of posting a separate notification per event.
pub(crate) fn notification_id_for(key: &str) -> i32 {
    let hash = key.bytes().fold(0u32, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(u32::from(b))
    }) & 0x7fff_ffff;
    i32::try_from(hash).unwrap_or(0)
}

/// JNI entry: `SilentPushBridge.nativeProcessSilentPush(String, String): String`.
///
/// Inputs are the app data directory path and the FCM data payload as a JSON
/// object (string → string). Output is the notification content as JSON
/// (`id`, `channelId`, `conversationTitle`, `selfName`, and a `messages` array of
/// `{ sender, personKey, text, timestamp, avatarBytes }`) for Kotlin to post, or
/// `null` on failure.
///
/// `data_dir` is the app's data directory (the same path Tauri's path API
/// resolves to on Android); a real client opens its on-disk store (e.g. the
/// Matrix SDK database) under it to decrypt the event.
///
/// # Safety
/// Called by the JVM with valid JNI references; not invoked from Rust.
#[no_mangle]
pub extern "system" fn Java_com_alexis_notiftestapp_SilentPushBridge_nativeProcessSilentPush<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    data_json: JString<'local>,
) -> jstring {
    match process(&mut env, &data_dir, &data_json) {
        Ok(json) => env
            .new_string(json)
            .map_or(std::ptr::null_mut(), jni::objects::JString::into_raw),
        Err(e) => {
            log::error!("nativeProcessSilentPush failed: {e}");
            std::ptr::null_mut()
        }
    }
}

fn process(env: &mut JNIEnv, data_dir: &JString, data_json: &JString) -> Result<String, String> {
    let data_dir: String = env
        .get_string(data_dir)
        .map_err(|e| format!("reading dataDir JString: {e}"))?
        .into();
    let input: String = env
        .get_string(data_json)
        .map_err(|e| format!("reading JString: {e}"))?
        .into();
    let data: HashMap<String, String> =
        serde_json::from_str(&input).map_err(|e| format!("parsing data JSON: {e}"))?;

    let room_id = data
        .get("room_id")
        .cloned()
        .unwrap_or_else(|| "!unknown:matrix.org".to_string());
    let event_id = data
        .get("event_id")
        .cloned()
        .unwrap_or_else(|| "$unknown".to_string());

    // A real client opens its Matrix SDK store under `data_dir` here, e.g.
    // `{data_dir}/matrix/<user>/db`, then decrypts the event via NotificationClient.
    log::info!(
        "silent push (background/JNI): fetching {event_id} in {room_id} (data dir: {data_dir})"
    );

    // Mirror a real async homeserver fetch on a short-lived runtime. This is the
    // seam where matrix-rust-sdk's NotificationClient would run.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("building runtime: {e}"))?;
    let (sender, body) = runtime.block_on(async {
        // e.g. notification_client.get_notification(&room_id, &event_id).await
        simulate_matrix_fetch(&room_id, &event_id)
    });

    // A real client would resolve the sender's matrix id and download their (or
    // the room's) avatar via matrix-sdk; here we derive a key and reuse the icon.
    let sender_key = format!("@{}:matrix.org", sender.to_lowercase());
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));

    // MessagingStyle: the plugin decodes `avatarBytes` and renders a chat-style
    // notification with the sender's circular avatar and the room as the title.
    // The id is keyed by the room, and `appendMessages` lets the plugin stack
    // each new event onto the same conversation notification.
    let out = serde_json::json!({
        "id": notification_id_for(&room_id),
        "channelId": "default",
        "title": sender,
        "body": body,
        "conversationTitle": room_id,
        "groupConversation": true,
        "selfName": "Me",
        "appendMessages": true,
        // Tapping the notification opens this Matrix deep link (ACTION_VIEW),
        // routed by the app's `matrix:` intent-filter to tauri-plugin-deep-link
        // (Option B). This replaces the `notificationClicked` event for the tap.
        "deepLink": matrix_uri(&room_id, &event_id),
        "messages": [{
            "sender": sender,
            "personKey": sender_key,
            "text": body,
            "timestamp": now_ms,
            "avatarBytes": demo_avatar_base64(),
        }],
    });
    Ok(out.to_string())
}
