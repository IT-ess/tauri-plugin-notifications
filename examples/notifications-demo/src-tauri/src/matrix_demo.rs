//! Platform-agnostic Matrix demo helpers shared by the silent-push paths:
//! Android's warm handler and killed-state JNI entry (`android_push.rs`), and
//! iOS's Notification Service Extension entry (`ios_push.rs`).
//!
//! These stand in for what a real Matrix client would do — fetch and decrypt an
//! event from its on-disk store and build the notification's identifiers. They
//! are pure (no platform APIs) so every entry point can reuse them.

// The helpers are `pub(crate)` for use from the sibling modules; this module is
// private, so clippy flags that as redundant — it isn't, the siblings need them.
#![allow(clippy::redundant_pub_crate)]

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
/// On Android a notification tap fires `ACTION_VIEW` for this URI (see
/// `android_push.rs`); on iOS it rides in the notification's `userInfo` (via
/// `extra`) and reaches JS through the `notificationClicked` event.
pub(crate) fn matrix_uri(room_id: &str, event_id: &str) -> String {
    let room = room_id.strip_prefix('!').unwrap_or(room_id);
    let event = event_id.strip_prefix('$').unwrap_or(event_id);
    format!("matrix:roomid/{room}/e/{event}")
}
