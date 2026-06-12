use serde::de::DeserializeOwned;
use tauri::{
    AppHandle, Runtime,
    plugin::{PermissionState, PluginApi, PluginHandle},
};

#[cfg(feature = "push-notifications")]
use crate::models::PushNotificationResponse;
use crate::models::{
    ActionType, ActiveNotification, Channel, PendingNotification, PermissionResponse,
};

use std::collections::HashMap;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.notification";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_notification);

// initializes the Kotlin or Swift plugin classes
// `PluginApi` is consumed by the framework's register helpers.
#[allow(clippy::needless_pass_by_value)]
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Notifications<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "NotificationPlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_notification)?;
    Ok(Notifications(handle))
}

impl<R: Runtime> crate::NotificationsBuilder<R> {
    pub async fn show(self) -> crate::Result<()> {
        self.handle
            .run_mobile_plugin_async::<i32>("show", self.data)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }
}

/// Access to the notification APIs.
///
/// You can get an instance of this type via [`NotificationExt`](crate::NotificationExt)
pub struct Notifications<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Notifications<R> {
    pub fn builder(&self) -> crate::NotificationsBuilder<R> {
        crate::NotificationsBuilder::new(self.0.clone())
    }

    pub async fn request_permission(&self) -> crate::Result<PermissionState> {
        self.0
            .run_mobile_plugin_async::<PermissionResponse>("requestPermissions", ())
            .await
            .map(|r| r.permission_state)
            .map_err(Into::into)
    }

    pub async fn register_for_push_notifications(&self) -> crate::Result<String> {
        #[cfg(feature = "push-notifications")]
        {
            self.0
                .run_mobile_plugin_async::<PushNotificationResponse>(
                    "registerForPushNotifications",
                    (),
                )
                .await
                .map(|r| r.device_token)
                .map_err(Into::into)
        }
        #[cfg(not(feature = "push-notifications"))]
        {
            Err(crate::Error::Io(std::io::Error::other(
                "Push notifications feature is not enabled",
            )))
        }
    }

    pub fn unregister_for_push_notifications(&self) -> crate::Result<()> {
        #[cfg(feature = "push-notifications")]
        {
            self.0
                .run_mobile_plugin::<()>("unregisterForPushNotifications", ())
                .map_err(Into::into)
        }
        #[cfg(not(feature = "push-notifications"))]
        {
            Err(crate::Error::Io(std::io::Error::other(
                "Push notifications feature is not enabled",
            )))
        }
    }

    pub async fn permission_state(&self) -> crate::Result<PermissionState> {
        self.0
            .run_mobile_plugin_async::<PermissionResponse>("checkPermissions", ())
            .await
            .map(|r| r.permission_state)
            .map_err(Into::into)
    }

    pub fn register_action_types(&self, types: Vec<ActionType>) -> crate::Result<()> {
        let mut args = HashMap::new();
        args.insert("types", types);
        self.0
            .run_mobile_plugin("registerActionTypes", args)
            .map_err(Into::into)
    }

    pub fn remove_active(&self, notifications: Vec<i32>) -> crate::Result<()> {
        let mut args = HashMap::new();
        args.insert(
            "notifications",
            notifications
                .into_iter()
                .map(|id| {
                    let mut notification = HashMap::new();
                    notification.insert("id", id);
                    notification
                })
                .collect::<Vec<HashMap<&str, i32>>>(),
        );
        self.0
            .run_mobile_plugin("removeActive", args)
            .map_err(Into::into)
    }

    pub async fn active(&self) -> crate::Result<Vec<ActiveNotification>> {
        self.0
            .run_mobile_plugin_async("getActive", ())
            .await
            .map_err(Into::into)
    }

    pub fn remove_all_active(&self) -> crate::Result<()> {
        self.0
            .run_mobile_plugin("removeActive", ())
            .map_err(Into::into)
    }

    pub async fn pending(&self) -> crate::Result<Vec<PendingNotification>> {
        self.0
            .run_mobile_plugin_async("getPending", ())
            .await
            .map_err(Into::into)
    }

    /// Cancel pending notifications.
    pub fn cancel(&self, notifications: Vec<i32>) -> crate::Result<()> {
        let mut args = HashMap::new();
        args.insert("notifications", notifications);
        self.0.run_mobile_plugin("cancel", args).map_err(Into::into)
    }

    /// Cancel all pending notifications.
    pub fn cancel_all(&self) -> crate::Result<()> {
        self.0
            .run_mobile_plugin("cancelAll", ())
            .map_err(Into::into)
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    pub fn create_channel(&self, channel: Channel) -> crate::Result<()> {
        #[cfg(target_os = "android")]
        return self
            .0
            .run_mobile_plugin("createChannel", channel)
            .map_err(Into::into);
        #[cfg(target_os = "ios")]
        return Err(crate::Error::Io(std::io::Error::other(
            "Channels are not supported on iOS",
        )));
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    pub fn delete_channel(&self, id: impl Into<String>) -> crate::Result<()> {
        #[cfg(target_os = "android")]
        {
            let mut args = HashMap::new();
            args.insert("id", id.into());
            self.0
                .run_mobile_plugin("deleteChannel", args)
                .map_err(Into::into)
        }
        #[cfg(target_os = "ios")]
        return Err(crate::Error::Io(std::io::Error::other(
            "Channels are not supported on iOS",
        )));
    }

    pub fn list_channels(&self) -> crate::Result<Vec<Channel>> {
        #[cfg(target_os = "android")]
        return self
            .0
            .run_mobile_plugin("listChannels", ())
            .map_err(Into::into);
        #[cfg(target_os = "ios")]
        return Err(crate::Error::Io(std::io::Error::other(
            "Channels are not supported on iOS",
        )));
    }

    /// Set click listener active state.
    /// Used internally to track if JS listener is registered.
    pub fn set_click_listener_active(&self, active: bool) -> crate::Result<()> {
        let mut args = HashMap::new();
        args.insert("active", active);
        self.0
            .run_mobile_plugin("setClickListenerActive", args)
            .map_err(Into::into)
    }

    /// Register a Rust-only handler for *silent* (data-only) push notifications
    /// on Android.
    ///
    /// A silent push is an FCM message delivered without a `notification` block:
    /// Android shows nothing and instead wakes the app with the raw data
    /// payload. This is what a Matrix client receives when the homeserver only
    /// sends an identifier (`room_id` / `event_id`) and expects the app to fetch
    /// the message body and raise the notification itself — typically by calling
    /// [`Notifications::builder`](Self::builder) from inside `handler`.
    ///
    /// `handler` runs whenever a silent push arrives **while the app process is
    /// alive** (foreground, or background but not yet killed by the OS). It is
    /// invoked on a background thread, so it must be `Send + Sync`. Only the most
    /// recently registered handler is active.
    ///
    /// This API is intentionally Rust-only — it is not exposed to the webview and
    /// has no JavaScript equivalent, because the fetch-then-display logic it
    /// drives belongs in the native layer.
    ///
    /// # Limitations
    ///
    /// If the OS has killed the app process, Android still starts the messaging
    /// service for a data message but the Tauri runtime — and therefore this
    /// handler — is not running, so the push is dropped. Delivering notifications
    /// in that state requires handling the message inside the Android service
    /// itself, which is outside the scope of this handler.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tauri::Runtime;
    /// # fn example<R: Runtime>(notifications: &tauri_plugin_notifications::Notifications<R>, app: tauri::AppHandle<R>) -> tauri_plugin_notifications::Result<()> {
    /// notifications.on_silent_push(move |push| {
    ///     // e.g. push.data.get("event_id") -> fetch from the homeserver -> show
    ///     log::info!("silent push: {:?}", push.data);
    /// })?;
    /// # Ok(()) }
    /// ```
    #[cfg(all(target_os = "android", feature = "push-notifications"))]
    pub fn on_silent_push<F>(&self, handler: F) -> crate::Result<()>
    where
        F: Fn(crate::models::SilentPushNotification) + Send + Sync + 'static,
    {
        use tauri::ipc::{Channel, InvokeResponseBody};

        // `TSend` is unused (we never call `channel.send` from Rust) but must be
        // nameable; the default `InvokeResponseBody` serializes to the channel id
        // the Kotlin side expects.
        let channel: Channel = Channel::new(move |event| {
            if let InvokeResponseBody::Json(payload) = event {
                match serde_json::from_str::<crate::models::SilentPushNotification>(&payload) {
                    Ok(push) => handler(push),
                    Err(e) => log::error!("failed to deserialize silent push payload: {e}"),
                }
            }
            Ok(())
        });

        let mut args = HashMap::new();
        args.insert("handler", channel);
        self.0
            .run_mobile_plugin("registerSilentPushHandler", args)
            .map_err(Into::into)
    }
}
