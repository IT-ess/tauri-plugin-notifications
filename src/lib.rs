//! Send message notifications (brief auto-expiring OS window element) to your user. Can also be used with the Notification Web API.

use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use tauri::AppHandle;
#[cfg(mobile)]
use tauri::plugin::PluginHandle;
use tauri::{
    Manager, Runtime,
    plugin::{Builder, TauriPlugin},
};

/// Top-level plugin config deserialized from the `plugins.notifications` block
/// in `tauri.conf.json`. Optional in its entirety — apps without a config block
/// get `PluginConfig::default()`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PluginConfig {
    #[cfg(target_os = "windows")]
    pub windows: WindowsConfig,
}

/// Windows-only plugin config.
///
/// Currently carries the toast activator CLSID used by
/// `INotificationActivationCallback` registration; absent value means
/// COM-based activation is disabled and the plugin falls back to in-process
/// `Activated` events only.
#[cfg(target_os = "windows")]
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowsConfig {
    /// Toast activator CLSID. Must match the GUID declared in the MSIX
    /// manifest's `<desktop:ToastNotificationActivation ToastActivatorCLSID>`
    /// and `<com:Class Id>` entries. Accepts the `xxxxxxxx-xxxx-...` form
    /// with or without surrounding braces.
    pub toast_activator_clsid: Option<String>,
}

pub use models::*;
pub use tauri::plugin::PermissionState;

#[cfg(all(desktop, any(feature = "notify-rust", target_os = "linux")))]
mod desktop;
#[cfg(all(target_os = "macos", not(feature = "notify-rust")))]
mod macos;
#[cfg(mobile)]
mod mobile;
#[cfg(all(desktop, target_os = "linux", feature = "push-notifications"))]
mod unifiedpush;
#[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
mod windows;

mod commands;
mod error;
#[cfg(desktop)]
mod listeners;
mod models;
pub mod nse;

pub use error::{Error, Result};
pub use nse::SilentPushResponse;

#[cfg(all(desktop, any(feature = "notify-rust", target_os = "linux")))]
pub use desktop::Notifications;
#[cfg(all(target_os = "macos", not(feature = "notify-rust")))]
pub use macos::Notifications;
#[cfg(mobile)]
pub use mobile::Notifications;
#[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
pub use windows::Notifications;

/// The notification builder.
#[derive(Debug)]
pub struct NotificationsBuilder<R: Runtime> {
    #[cfg(desktop)]
    #[allow(dead_code)]
    app: AppHandle<R>,
    #[cfg(all(target_os = "macos", not(feature = "notify-rust")))]
    plugin: std::sync::Arc<macos::NotificationPlugin>,
    #[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
    plugin: std::sync::Arc<windows::WindowsPlugin>,
    #[cfg(mobile)]
    handle: PluginHandle<R>,
    pub(crate) data: NotificationData,
}

impl<R: Runtime> NotificationsBuilder<R> {
    #[cfg(all(desktop, any(feature = "notify-rust", target_os = "linux")))]
    fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            data: NotificationData::default(),
        }
    }

    #[cfg(all(target_os = "macos", not(feature = "notify-rust")))]
    fn new(app: AppHandle<R>, plugin: std::sync::Arc<macos::NotificationPlugin>) -> Self {
        Self {
            app,
            plugin,
            data: NotificationData::default(),
        }
    }

    #[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
    fn new(app: AppHandle<R>, plugin: std::sync::Arc<windows::WindowsPlugin>) -> Self {
        Self {
            app,
            plugin,
            data: Default::default(),
        }
    }

    #[cfg(mobile)]
    fn new(handle: PluginHandle<R>) -> Self {
        Self {
            handle,
            data: NotificationData::default(),
        }
    }

    crate::models::notification_setters!();

    /// Replaces the entire payload with an already-built [`NotificationData`]
    /// — e.g. one produced by a shared silent-push handler
    /// ([`NotificationData::builder`]) that the app also wants to show while
    /// running.
    #[must_use]
    pub fn data(mut self, data: NotificationData) -> Self {
        self.data = data;
        self
    }
}

/// Extensions to [`tauri::App`], [`tauri::AppHandle`], [`tauri::WebviewWindow`], [`tauri::Webview`] and [`tauri::Window`] to access the notification APIs.
pub trait NotificationsExt<R: Runtime> {
    fn notifications(&self) -> &Notifications<R>;
}

impl<R: Runtime, T: Manager<R>> crate::NotificationsExt<R> for T {
    fn notifications(&self) -> &Notifications<R> {
        self.state::<Notifications<R>>().inner()
    }
}

/// Initializes the plugin.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R, Option<PluginConfig>> {
    Builder::<R, Option<PluginConfig>>::new("notifications")
        .invoke_handler(tauri::generate_handler![
            commands::notify,
            commands::request_permission,
            commands::register_for_push_notifications,
            commands::unregister_for_push_notifications,
            commands::is_permission_granted,
            commands::register_action_types,
            commands::get_pending,
            commands::get_active,
            commands::set_click_listener_active,
            commands::remove_active,
            commands::remove_all,
            commands::cancel,
            commands::cancel_all,
            commands::create_channel,
            commands::delete_channel,
            commands::list_channels,
            #[cfg(desktop)]
            listeners::register_listener,
            #[cfg(desktop)]
            listeners::remove_listener,
            #[cfg(all(desktop, target_os = "linux", feature = "push-notifications"))]
            commands::list_distributors,
            #[cfg(all(desktop, target_os = "linux", feature = "push-notifications"))]
            commands::set_distributor,
            #[cfg(all(desktop, target_os = "linux", feature = "push-notifications"))]
            commands::set_token,
        ])
        .setup(|app, api| {
            #[cfg(desktop)]
            listeners::init();
            #[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
            let windows_config = api
                .config()
                .as_ref()
                .map(|c| c.windows.clone())
                .unwrap_or_default();
            #[cfg(mobile)]
            let notification = mobile::init(app, api)?;
            #[cfg(all(desktop, any(feature = "notify-rust", target_os = "linux")))]
            let notification = desktop::init(app, api)?;
            #[cfg(all(target_os = "macos", not(feature = "notify-rust")))]
            let notification = macos::init(app, api)?;
            #[cfg(all(target_os = "windows", not(feature = "notify-rust")))]
            let notification = windows::init(app, api, windows_config)?;
            app.manage(notification);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test builder without needing a runtime
    #[cfg(desktop)]
    fn create_test_data() -> NotificationData {
        NotificationData::default()
    }

    #[cfg(mobile)]
    fn create_test_data() -> NotificationData {
        NotificationData::default()
    }

    #[test]
    fn test_notification_data_id() {
        let mut data = create_test_data();
        data.id = 42;
        assert_eq!(data.id, 42);
    }

    #[test]
    fn test_notification_data_channel_id() {
        let mut data = create_test_data();
        data.channel_id = Some("test_channel".to_string());
        assert_eq!(data.channel_id, Some("test_channel".to_string()));
    }

    #[test]
    fn test_notification_data_title() {
        let mut data = create_test_data();
        data.title = Some("Test Title".to_string());
        assert_eq!(data.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_notification_data_body() {
        let mut data = create_test_data();
        data.body = Some("Test Body".to_string());
        assert_eq!(data.body, Some("Test Body".to_string()));
    }

    #[test]
    fn test_notification_data_large_body() {
        let mut data = create_test_data();
        data.large_body = Some("Large Body Text".to_string());
        assert_eq!(data.large_body, Some("Large Body Text".to_string()));
    }

    #[test]
    fn test_notification_data_summary() {
        let mut data = create_test_data();
        data.summary = Some("Summary Text".to_string());
        assert_eq!(data.summary, Some("Summary Text".to_string()));
    }

    #[test]
    fn test_notification_data_action_type_id() {
        let mut data = create_test_data();
        data.action_type_id = Some("action_type".to_string());
        assert_eq!(data.action_type_id, Some("action_type".to_string()));
    }

    #[test]
    fn test_notification_data_group() {
        let mut data = create_test_data();
        data.group = Some("test_group".to_string());
        assert_eq!(data.group, Some("test_group".to_string()));
    }

    #[test]
    fn test_notification_data_group_summary() {
        let mut data = create_test_data();
        data.group_summary = true;
        assert!(data.group_summary);
    }

    #[test]
    fn test_notification_data_sound() {
        let mut data = create_test_data();
        data.sound = Some("notification_sound".to_string());
        assert_eq!(data.sound, Some("notification_sound".to_string()));
    }

    #[test]
    fn test_notification_data_inbox_lines() {
        let mut data = create_test_data();
        data.inbox_lines.push("Line 1".to_string());
        data.inbox_lines.push("Line 2".to_string());
        assert_eq!(data.inbox_lines.len(), 2);
        assert_eq!(data.inbox_lines[0], "Line 1");
        assert_eq!(data.inbox_lines[1], "Line 2");
    }

    #[test]
    fn test_notification_data_icon() {
        let mut data = create_test_data();
        data.icon = Some("icon_name".to_string());
        assert_eq!(data.icon, Some("icon_name".to_string()));
    }

    #[test]
    fn test_notification_data_large_icon() {
        let mut data = create_test_data();
        data.large_icon = Some("large_icon_name".to_string());
        assert_eq!(data.large_icon, Some("large_icon_name".to_string()));
    }

    #[test]
    fn test_notification_data_icon_color() {
        let mut data = create_test_data();
        data.icon_color = Some("#FF0000".to_string());
        assert_eq!(data.icon_color, Some("#FF0000".to_string()));
    }

    #[test]
    fn test_notification_data_attachments() {
        let mut data = create_test_data();
        let url = url::Url::parse("https://example.com/image.png").expect("Failed to parse URL");
        let attachment = Attachment::new("attachment1", url);
        data.attachments.push(attachment);
        assert_eq!(data.attachments.len(), 1);
    }

    #[test]
    fn test_notification_data_extra() {
        let mut data = create_test_data();
        data.extra
            .insert("key1".to_string(), serde_json::json!("value1"));
        data.extra.insert("key2".to_string(), serde_json::json!(42));
        assert_eq!(data.extra.len(), 2);
        assert_eq!(data.extra.get("key1"), Some(&serde_json::json!("value1")));
        assert_eq!(data.extra.get("key2"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_notification_data_ongoing() {
        let mut data = create_test_data();
        data.ongoing = true;
        assert!(data.ongoing);
    }

    #[test]
    fn test_notification_data_auto_cancel() {
        let mut data = create_test_data();
        data.auto_cancel = true;
        assert!(data.auto_cancel);
    }

    #[test]
    fn test_notification_data_silent() {
        let mut data = create_test_data();
        data.silent = true;
        assert!(data.silent);
    }

    #[test]
    fn test_notification_data_schedule() {
        let mut data = create_test_data();
        let schedule = Schedule::Every {
            interval: ScheduleEvery::Day,
            count: 1,
            allow_while_idle: false,
        };
        data.schedule = Some(schedule);
        assert!(data.schedule.is_some());
        assert!(matches!(data.schedule, Some(Schedule::Every { .. })));
    }
}
