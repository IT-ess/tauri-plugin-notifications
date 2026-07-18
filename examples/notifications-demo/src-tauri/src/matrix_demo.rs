//! Platform-agnostic Matrix demo helpers shared by the silent-push handler
//! (`push_handler.rs`) and the warm simulate command (`lib.rs`).
//!
//! These stand in for what a real Matrix client would do — fetch and decrypt an
//! event from its on-disk store and build the notification's identifiers. They
//! are pure (no platform APIs) so every entry point can reuse them.

// The helpers are `pub(crate)` for use from the sibling modules; this module is
// private, so clippy flags that as redundant — it isn't, the siblings need them.
#![allow(clippy::redundant_pub_crate)]

use std::sync::LazyLock;

use base64::Engine;

/// Base64-encoded demo avatar, encoded once. Stands in for the bytes a real
/// client gets from matrix-sdk's media store after downloading the sender/room
/// `mxc://` avatar; here we just reuse the app icon so no extra asset is
/// committed. (Re-sending it per message is also demo-only: the plugin
/// persists avatars by `person_key`, so a real client sends them once.)
pub(crate) static DEMO_AVATAR: LazyLock<String> = LazyLock::new(|| {
    base64::engine::general_purpose::STANDARD.encode(include_bytes!("../icons/testavatar.png"))
});

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

/// Derive the sender's stable person key. A real client would resolve the
/// actual Matrix user id; the demo fakes one from the display name.
pub(crate) fn sender_key(sender: &str) -> String {
    format!("@{}:matrix.org", sender.to_lowercase())
}

/// Pretend to fetch the event body from a homeserver. Returns `(sender, body)`.
///
/// In a real client this is where `matrix_sdk::NotificationClient` would load
/// and decrypt the event. The body is intentionally long so the expandable
/// notification has something to show.
pub(crate) fn simulate_matrix_fetch(room_id: &str, event_id: &str) -> (String, String) {
    (
        "Alice".to_string(),
        format!(
            "Hey! Are you around later to review the PR? I pushed the fix we \
             discussed and added a couple of tests. (room {room_id}, event {event_id})"
        ),
    )
}

/// Build the canonical Matrix URI (MSC2312) for an event in a room, e.g.
/// `matrix:roomid/abc:matrix.org/e/xyz` from `!abc:matrix.org` / `$xyz`. Sigils
/// (`!`/`$`) are dropped; the spec keeps `:` literal in the path.
///
/// On Android a notification tap fires `ACTION_VIEW` for this URI (the
/// `deep_link` field); on iOS it rides in the notification's `userInfo` (via
/// `extra`) and reaches JS through the `notificationClicked` event.
pub(crate) fn matrix_uri(room_id: &str, event_id: &str) -> String {
    let room = room_id.strip_prefix('!').unwrap_or(room_id);
    let event = event_id.strip_prefix('$').unwrap_or(event_id);
    format!("matrix:roomid/{room}/e/{event}")
}
