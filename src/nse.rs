//! Killed-state silent-push support: the iOS Notification Service Extension
//! (NSE) and the Android cold-start path share one Rust handler.
//!
//! iOS delivers a Matrix-style push (`mutable-content: 1` plus a fallback
//! `alert`) to the host app's Notification Service Extension in every app
//! state — including after a force-quit. Android instead cold-starts the
//! plugin's `TauriFirebaseMessagingService` for a data-only message. Both run
//! without a Tauri runtime, so the decode step runs in the host app's own
//! native library, reached through a fixed symbol the plugin resolves:
//!
//! * **iOS/macOS** — the plugin's Swift `TauriNotificationService` (product
//!   `tauri-plugin-notifications-nse` in `ios/Package.swift`) looks up the C
//!   symbol `tauri_notifications_process_silent_push` via `dlsym` in the
//!   staticlib the NSE target links (`libapp.a`).
//! * **Android** — the plugin's messaging service loads the app's native
//!   library (named by the `app.tauri.notification.SILENT_PUSH_LIB`
//!   manifest meta-data), calls the JNI export backing the plugin's
//!   `SilentPushNative.process`, and posts the returned notification itself.
//!
//! The host app provides both exports by invoking
//! [`silent_push_handler!`](crate::silent_push_handler) once with a single
//! platform-neutral handler:
//!
//! ```ignore
//! // src-tauri/src/push_handler.rs — compiled for iOS and Android
//! use std::collections::HashMap;
//! use tauri_plugin_notifications::{NotificationData, SilentPushResponse};
//!
//! fn handle_silent_push(
//!     data_dir: &str,
//!     data: HashMap<String, String>,
//! ) -> SilentPushResponse {
//!     let (Some(room_id), Some(event_id)) = (data.get("room_id"), data.get("event_id"))
//!     else {
//!         return SilentPushResponse::Decline;
//!     };
//!     // Fetch/decrypt the event from the store under `data_dir`
//!     // (App Group container on iOS, the app data dir on Android), then:
//!     SilentPushResponse::Notification(
//!         NotificationData::builder()
//!             .title("Alice")
//!             .body("decrypted message")
//!             .group(room_id)
//!             .extra("deepLink", format!("matrix:roomid/{room_id}/e/{event_id}"))
//!             .build(),
//!     )
//! }
//!
//! tauri_plugin_notifications::silent_push_handler!(handle_silent_push);
//! ```
//!
//! Declining (or panicking, or exceeding the platform's budget — ~30 s on
//! iOS) never drops a notification: iOS delivers the push's original content
//! (its fallback `alert`), and Android falls through to the JS `push-message`
//! event.
//!
//! On Android the process the handler runs in may have been started *only*
//! for the push, skipping every process-wide initialization the app normally
//! performs at startup (TLS roots, keyring backend, `ndk_context`, …). Pass a
//! second path to the macro to replay them before the handler runs:
//!
//! ```ignore
//! fn android_init(env: &mut jni::JNIEnv, context: &jni::objects::JObject) {
//!     // `context` is the Application context the messaging service runs in.
//! }
//!
//! tauri_plugin_notifications::silent_push_handler!(handle_silent_push, android_init = android_init);
//! ```

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::models::NotificationData;

/// Re-exported for the JNI export `silent_push_handler!` generates; not public API.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub use jni;

/// Outcome of a silent-push handler passed to
/// [`silent_push_handler!`](crate::silent_push_handler).
// One short-lived value per push: boxing the notification would only push the
// indirection onto every consumer for no measurable win.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum SilentPushResponse {
    /// Display this notification.
    Notification(NotificationData),
    /// Post nothing, and dismiss every active notification along with the
    /// plugin's stored conversation histories — for pushes that mean "all
    /// messages have been read elsewhere" (e.g. a Matrix badge push with
    /// `unread == 0`). Android only: on iOS such pushes are delivered as
    /// background pushes that never reach the NSE, so this is treated as
    /// [`Decline`](Self::Decline) there.
    ClearActive,
    /// Decline the push: iOS keeps its original fallback content; Android
    /// falls through to the JS `push-message` event.
    Decline,
}

/// Signature of a host silent-push handler passed to
/// [`silent_push_handler!`](crate::silent_push_handler).
///
/// * `data_dir` — the directory for shared app data: on iOS the App Group
///   container (see the Swift side's `TauriNotificationsAppGroup` Info.plist
///   key); on Android the app data directory (what Tauri's path API resolves).
/// * `data` — the push payload's custom keys, stringified: on iOS everything
///   except `aps`, on Android the FCM data payload.
pub type SilentPushHandler = fn(&str, HashMap<String, String>) -> SilentPushResponse;

/// Platform-neutral core: parse the payload JSON and run the handler with
/// panics contained. Any failure degrades to `Decline`; the cause is logged.
fn run_handler(handler: SilentPushHandler, data_dir: &str, data_json: &str) -> SilentPushResponse {
    let data: HashMap<String, String> = match serde_json::from_str(data_json) {
        Ok(data) => data,
        Err(e) => {
            log::error!("failed to parse silent push data JSON: {e}");
            return SilentPushResponse::Decline;
        }
    };

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(data_dir, data)))
        .unwrap_or_else(|_| {
            log::error!("silent push handler panicked");
            SilentPushResponse::Decline
        })
}

/// Serializes a notification for the platform bridge, degrading to `None`
/// (decline) if serialization ever fails.
fn notification_to_json(notification: &NotificationData) -> Option<String> {
    match serde_json::to_string(notification) {
        Ok(json) => Some(json),
        Err(e) => {
            log::error!("failed to serialize notification: {e}");
            None
        }
    }
}

/// Backing implementation for the `tauri_notifications_process_silent_push`
/// symbol generated by [`silent_push_handler!`](crate::silent_push_handler).
///
/// Returns the handler's [`NotificationData`] serialized as a C string
/// allocated with [`CString::into_raw`], or null when the inputs are invalid,
/// the handler returns `None`, or it panics. A non-null return must be
/// released exactly once with [`free`].
///
/// # Safety
///
/// `data_dir` and `data_json` must be null or valid NUL-terminated C strings
/// that outlive this call.
#[doc(hidden)]
pub unsafe fn process(
    handler: SilentPushHandler,
    data_dir: *const c_char,
    data_json: *const c_char,
) -> *mut c_char {
    if data_dir.is_null() || data_json.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: non-null, and the caller guarantees valid NUL-terminated strings.
    let (data_dir, data_json) = unsafe { (CStr::from_ptr(data_dir), CStr::from_ptr(data_json)) };
    let (Ok(data_dir), Ok(data_json)) = (data_dir.to_str(), data_json.to_str()) else {
        log::error!("silent push input is not valid UTF-8");
        return std::ptr::null_mut();
    };

    // `ClearActive` has no NSE meaning (see `SilentPushResponse`): decline so
    // the extension keeps the push's own content.
    let json = match run_handler(handler, data_dir, data_json) {
        SilentPushResponse::Notification(notification) => notification_to_json(&notification),
        SilentPushResponse::ClearActive | SilentPushResponse::Decline => None,
    };
    json.map_or(std::ptr::null_mut(), |json| {
        // JSON never contains interior NULs, but don't panic if that ever changes.
        CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw)
    })
}

/// Signature of the Android pre-handler init hook (the optional
/// `android_init` argument of
/// [`silent_push_handler!`](crate::silent_push_handler)).
///
/// Receives the JNI env of the messaging service's calling thread and the
/// Application context, before the handler runs — the place to replay
/// process-wide initializations a cold-started push process is missing (TLS
/// roots, keyring, `ndk_context`). Must be idempotent: it runs once per push,
/// warm or cold.
#[cfg(target_os = "android")]
pub type SilentPushAndroidInit = fn(&mut jni::JNIEnv, &jni::objects::JObject);

/// Default `android_init`: nothing to replay.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub const fn default_android_init(_env: &mut jni::JNIEnv, _context: &jni::objects::JObject) {}

/// Backing implementation for the JNI export
/// `Java_app_tauri_notification_SilentPushNative_process` generated by
/// [`silent_push_handler!`](crate::silent_push_handler): the Android
/// counterpart of [`process`], called by the plugin's messaging service for
/// data-only pushes (including after a cold start). Runs `android_init`
/// first, then the handler. Returns the [`NotificationData`] JSON as a Java
/// string, the `{"clearAll":true}` directive for
/// [`SilentPushResponse::ClearActive`], or null to decline.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn process_jni(
    handler: SilentPushHandler,
    android_init: SilentPushAndroidInit,
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
    data_dir: &jni::objects::JString,
    data_json: &jni::objects::JString,
) -> jni::sys::jstring {
    // Contain init panics (unwinding across the JNI boundary aborts the
    // process) but keep going: a handler running without its init typically
    // degrades to its own fallback content, which is the debuggable symptom.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| android_init(env, context)))
        .is_err()
    {
        log::error!("silent push android_init panicked");
    }

    let data_dir: String = match env.get_string(data_dir) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!("silent push: reading dataDir JString failed: {e}");
            return std::ptr::null_mut();
        }
    };
    let data_json: String = match env.get_string(data_json) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!("silent push: reading payload JString failed: {e}");
            return std::ptr::null_mut();
        }
    };

    let json = match run_handler(handler, &data_dir, &data_json) {
        SilentPushResponse::Notification(notification) => notification_to_json(&notification),
        // The messaging service intercepts this directive before parsing a
        // notification (see TauriFirebaseMessagingService.processNative).
        SilentPushResponse::ClearActive => Some(r#"{"clearAll":true}"#.to_string()),
        SilentPushResponse::Decline => None,
    };
    json.map_or(std::ptr::null_mut(), |json| {
        env.new_string(json)
            .map_or(std::ptr::null_mut(), jni::objects::JString::into_raw)
    })
}

/// Backing implementation for the `tauri_notifications_silent_push_free`
/// symbol generated by [`silent_push_handler!`](crate::silent_push_handler).
///
/// # Safety
///
/// `ptr` must be null or a pointer previously returned by [`process`] that has
/// not been freed yet.
#[doc(hidden)]
pub unsafe fn free(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: per contract, `ptr` came from `CString::into_raw` in
        // `process` and is released here exactly once.
        drop(unsafe { CString::from_raw(ptr) });
    }
}

/// Exports the killed-state silent-push entry points for a host handler of
/// type [`SilentPushHandler`](crate::nse::SilentPushHandler), covering both
/// mobile platforms with one invocation.
///
/// Invoke once, at the crate root or inside a mobile-only module of your
/// `src-tauri` crate. See the [`nse`](crate::nse) module docs for the full
/// walkthrough and an example.
///
/// The generated symbols are:
///
/// * `tauri_notifications_process_silent_push(data_dir, data_json) -> char*` —
///   resolved via `dlsym` by the plugin's iOS Notification Service Extension;
///   returns the [`NotificationData`](crate::NotificationData) JSON to display,
///   or null to keep the push's fallback content.
/// * `tauri_notifications_silent_push_free(char*)` — releases a returned string.
/// * `Java_app_tauri_notification_SilentPushNative_process` (Android only) —
///   the JNI method behind the plugin's `SilentPushNative.process`, called by
///   the plugin's messaging service for data-only pushes. Requires the
///   `app.tauri.notification.SILENT_PUSH_LIB` manifest meta-data naming the
///   app's native library.
///
/// The optional `android_init = path` argument names a
/// [`SilentPushAndroidInit`](crate::nse::SilentPushAndroidInit) hook that runs
/// on Android before every handler invocation, with the JNI env and the
/// Application context — the place to replay process-wide initializations a
/// cold-started push process is missing. The path is only referenced from
/// Android-only code, so it may be an `#[cfg(target_os = "android")]` item:
///
/// ```ignore
/// tauri_plugin_notifications::silent_push_handler!(handle_silent_push, android_init = my_init);
/// ```
#[macro_export]
macro_rules! silent_push_handler {
    ($handler:path) => {
        $crate::silent_push_handler!(@impl $handler, $crate::nse::default_android_init);
    };
    ($handler:path, android_init = $init:path) => {
        $crate::silent_push_handler!(@impl $handler, $init);
    };
    (@impl $handler:path, $init:path) => {
        /// Called by the plugin's Notification Service Extension via `dlsym`.
        ///
        /// # Safety
        ///
        /// The arguments must be null or valid NUL-terminated C strings; the
        /// non-null return value must be released exactly once with
        /// `tauri_notifications_silent_push_free`.
        #[unsafe(no_mangle)]
        pub extern "C" fn tauri_notifications_process_silent_push(
            data_dir: *const ::std::os::raw::c_char,
            data_json: *const ::std::os::raw::c_char,
        ) -> *mut ::std::os::raw::c_char {
            let handler: $crate::nse::SilentPushHandler = $handler;
            // SAFETY: the extension passes valid NUL-terminated C strings that
            // outlive the call.
            unsafe { $crate::nse::process(handler, data_dir, data_json) }
        }

        /// Releases a string returned by `tauri_notifications_process_silent_push`.
        ///
        /// # Safety
        ///
        /// `ptr` must be null or an unreleased return value of
        /// `tauri_notifications_process_silent_push`.
        #[unsafe(no_mangle)]
        pub extern "C" fn tauri_notifications_silent_push_free(ptr: *mut ::std::os::raw::c_char) {
            // SAFETY: per contract, `ptr` is null or came from `process` and is
            // released exactly once.
            unsafe { $crate::nse::free(ptr) }
        }

        /// Called by the plugin's `SilentPushNative` Kotlin object after the
        /// messaging service loads this library (`SILENT_PUSH_LIB` meta-data).
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_app_tauri_notification_SilentPushNative_process<'local>(
            mut env: $crate::nse::jni::JNIEnv<'local>,
            _class: $crate::nse::jni::objects::JClass<'local>,
            context: $crate::nse::jni::objects::JObject<'local>,
            data_dir: $crate::nse::jni::objects::JString<'local>,
            data_json: $crate::nse::jni::objects::JString<'local>,
        ) -> $crate::nse::jni::sys::jstring {
            let handler: $crate::nse::SilentPushHandler = $handler;
            let init: $crate::nse::SilentPushAndroidInit = $init;
            $crate::nse::process_jni(handler, init, &mut env, &context, &data_dir, &data_json)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // The by-value map is the `SilentPushHandler` contract (hosts own the data).
    #[allow(clippy::needless_pass_by_value)]
    fn demo_handler(data_dir: &str, data: HashMap<String, String>) -> SilentPushResponse {
        if data.contains_key("clear") {
            return SilentPushResponse::ClearActive;
        }
        let Some(room_id) = data.get("room_id") else {
            return SilentPushResponse::Decline;
        };
        SilentPushResponse::Notification(
            NotificationData::builder()
                .id(1)
                .title("Alice")
                .body(format!("event in {room_id} (store: {data_dir})"))
                .group(room_id)
                .badge(3)
                .extra("deepLink", format!("matrix:roomid/{room_id}"))
                .build(),
        )
    }

    crate::silent_push_handler!(demo_handler);

    fn call(data_dir: &str, data_json: &str) -> Option<serde_json::Value> {
        let data_dir = CString::new(data_dir).expect("data_dir contains NUL");
        let data_json = CString::new(data_json).expect("data_json contains NUL");
        let ptr = tauri_notifications_process_silent_push(data_dir.as_ptr(), data_json.as_ptr());
        if ptr.is_null() {
            return None;
        }
        // SAFETY: non-null pointers from `process` are valid C strings until freed.
        let json = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .expect("returned JSON is not UTF-8")
            .to_string();
        tauri_notifications_silent_push_free(ptr);
        Some(serde_json::from_str(&json).expect("returned string is not valid JSON"))
    }

    #[test]
    fn round_trips_notification_json() {
        let json = call(
            "/data",
            r#"{"room_id":"!abc:matrix.org","event_id":"$xyz"}"#,
        )
        .expect("handler returned null");
        assert_eq!(json["id"], 1);
        assert_eq!(json["title"], "Alice");
        assert_eq!(json["group"], "!abc:matrix.org");
        // Pins the camelCase wire key the NSE Swift decoder reads.
        assert_eq!(json["badge"], 3);
        assert_eq!(json["extra"]["deepLink"], "matrix:roomid/!abc:matrix.org");
        assert_eq!(json["body"], "event in !abc:matrix.org (store: /data)");
    }

    #[test]
    fn returns_null_when_handler_declines() {
        // No room_id → the demo handler declines → fallback content.
        assert!(call("/data", r#"{"other":"key"}"#).is_none());
    }

    #[test]
    fn clear_active_declines_on_the_nse_path() {
        // ClearActive is an Android-only directive; the C (NSE) entry point
        // maps it to null so iOS keeps the push's fallback content.
        assert!(call("/data", r#"{"clear":"1"}"#).is_none());
    }

    #[test]
    fn returns_null_on_malformed_json() {
        assert!(call("/data", "not json").is_none());
        assert!(call("/data", r#"{"nested":{"x":1}}"#).is_none());
    }

    #[test]
    fn returns_null_on_null_inputs() {
        let ptr = tauri_notifications_process_silent_push(std::ptr::null(), std::ptr::null());
        assert!(ptr.is_null());
        // Freeing null is a no-op.
        tauri_notifications_silent_push_free(std::ptr::null_mut());
    }
}
