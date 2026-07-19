use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use tauri::plugin::PermissionState;

use url::Url;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub permission_state: PermissionState,
}

#[cfg(feature = "push-notifications")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotificationResponse {
    pub device_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    id: String,
    url: Url,
}

impl Attachment {
    pub fn new(id: impl Into<String>, url: Url) -> Self {
        Self { id: id.into(), url }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInterval {
    pub year: Option<u8>,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub weekday: Option<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    pub second: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum ScheduleEvery {
    Year,
    Month,
    TwoWeeks,
    Week,
    Day,
    Hour,
    Minute,
    Second,
}

impl Display for ScheduleEvery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Year => "year",
                Self::Month => "month",
                Self::TwoWeeks => "twoWeeks",
                Self::Week => "week",
                Self::Day => "day",
                Self::Hour => "hour",
                Self::Minute => "minute",
                Self::Second => "second",
            }
        )
    }
}

impl Serialize for ScheduleEvery {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

impl<'de> Deserialize<'de> for ScheduleEvery {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "year" => Ok(Self::Year),
            "month" => Ok(Self::Month),
            "twoweeks" => Ok(Self::TwoWeeks),
            "week" => Ok(Self::Week),
            "day" => Ok(Self::Day),
            "hour" => Ok(Self::Hour),
            "minute" => Ok(Self::Minute),
            "second" => Ok(Self::Second),
            _ => Err(DeError::custom(format!("unknown every kind '{s}'"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    #[serde(rename_all = "camelCase")]
    At {
        #[serde(
            serialize_with = "iso8601::serialize",
            deserialize_with = "time::serde::iso8601::deserialize"
        )]
        date: time::OffsetDateTime,
        #[serde(default)]
        repeating: bool,
        #[serde(default)]
        allow_while_idle: bool,
    },
    #[serde(rename_all = "camelCase")]
    Interval {
        interval: ScheduleInterval,
        #[serde(default)]
        allow_while_idle: bool,
    },
    #[serde(rename_all = "camelCase")]
    Every {
        interval: ScheduleEvery,
        count: u8,
        #[serde(default)]
        allow_while_idle: bool,
    },
}

// custom ISO-8601 serialization that does not use 6 digits for years.
mod iso8601 {
    use serde::{Serialize, Serializer, ser::Error as _};
    use time::{
        OffsetDateTime,
        format_description::well_known::Iso8601,
        format_description::well_known::iso8601::{Config, EncodedConfig},
    };

    const SERDE_CONFIG: EncodedConfig = Config::DEFAULT.encode();

    pub fn serialize<S: Serializer>(
        datetime: &OffsetDateTime,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        datetime
            .format(&Iso8601::<SERDE_CONFIG>)
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

// Each bool is an independent flag in the JS wire format; grouping them would change the JSON shape.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationData {
    #[serde(default = "default_id")]
    pub(crate) id: i32,
    pub(crate) channel_id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) schedule: Option<Schedule>,
    pub(crate) large_body: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) action_type_id: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(default)]
    pub(crate) group_summary: bool,
    pub(crate) sound: Option<String>,
    #[serde(default)]
    pub(crate) inbox_lines: Vec<String>,
    pub(crate) icon: Option<String>,
    pub(crate) large_icon: Option<String>,
    pub(crate) icon_color: Option<String>,
    #[serde(default)]
    pub(crate) attachments: Vec<Attachment>,
    #[serde(default)]
    pub(crate) extra: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub(crate) ongoing: bool,
    #[serde(default)]
    pub(crate) auto_cancel: bool,
    #[serde(default)]
    pub(crate) silent: bool,
    /// Chat messages rendered with Android `MessagingStyle` (per-sender avatars).
    /// When non-empty this takes precedence over `largeBody` / `inboxLines`.
    /// On iOS (silent-push NSE path) the last message upgrades the delivery to
    /// a communication notification: the sender's avatar replaces the app icon.
    #[serde(default)]
    pub(crate) messages: Vec<NotificationMessage>,
    pub(crate) conversation_title: Option<String>,
    #[serde(default)]
    pub(crate) group_conversation: bool,
    /// Avatar of the group conversation (room) as base64-encoded image bytes.
    /// With `group_conversation`, it is drawn as the notification icon instead
    /// of the sender's avatar: communication-notification icon on iOS (NSE),
    /// `MessagingStyle` conversation icon (largeIcon) on Android.
    pub(crate) conversation_avatar_bytes: Option<String>,
    pub(crate) self_name: Option<String>,
    /// When true (default), posting a `MessagingStyle` notification whose `id` is
    /// already showing appends the new messages to that conversation instead of
    /// replacing it. Android only.
    #[serde(default = "default_true")]
    pub(crate) append_messages: bool,
    /// Android only. When set, tapping the notification fires an `ACTION_VIEW`
    /// intent for this URI (a deep link, e.g. `matrix:roomid/…`) pinned to the
    /// app's own package, instead of launching the default activity. The app's
    /// matching `<intent-filter>` (e.g. via `tauri-plugin-deep-link`) then
    /// receives it. Replaces the `notificationClicked` event for that tap.
    pub(crate) deep_link: Option<String>,
    /// App icon badge count applied when this notification is delivered (iOS
    /// silent-push NSE path only). `None` leaves the current badge untouched.
    pub(crate) badge: Option<i32>,
}

const fn default_true() -> bool {
    true
}

/// A single chat message for an Android `MessagingStyle` notification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMessage {
    /// Display name of the sender (`None` renders as the local user).
    pub(crate) sender: Option<String>,
    /// Stable key identifying the sender (e.g. a Matrix user id), used by the
    /// system to de-duplicate/merge senders across messages.
    pub(crate) person_key: Option<String>,
    /// Sender avatar as base64-encoded image bytes; shown as a circular icon.
    pub(crate) avatar_bytes: Option<String>,
    /// Message text.
    pub(crate) text: Option<String>,
    /// Message time in epoch milliseconds (`0` → now).
    #[serde(default)]
    pub(crate) timestamp: i64,
}

impl NotificationMessage {
    /// Create a message with the given text. Defaults: no sender (rendered as the
    /// local user), no avatar, timestamp `0` (rendered as "now").
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    /// Set the sender's display name.
    #[must_use]
    pub fn sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = Some(sender.into());
        self
    }

    /// Set a stable key identifying the sender (e.g. a Matrix user id).
    #[must_use]
    pub fn person_key(mut self, key: impl Into<String>) -> Self {
        self.person_key = Some(key.into());
        self
    }

    /// Set the sender avatar as base64-encoded image bytes (PNG/JPEG).
    #[must_use]
    pub fn avatar_bytes(mut self, base64: impl Into<String>) -> Self {
        self.avatar_bytes = Some(base64.into());
        self
    }

    /// Set the message time in epoch milliseconds.
    #[must_use]
    pub const fn timestamp(mut self, millis: i64) -> Self {
        self.timestamp = millis;
        self
    }
}

fn default_id() -> i32 {
    rand::random()
}

impl Default for NotificationData {
    fn default() -> Self {
        Self {
            id: default_id(),
            channel_id: None,
            title: None,
            body: None,
            schedule: None,
            large_body: None,
            summary: None,
            action_type_id: None,
            group: None,
            group_summary: false,
            sound: None,
            inbox_lines: Vec::new(),
            icon: None,
            large_icon: None,
            icon_color: None,
            attachments: Vec::new(),
            extra: HashMap::default(),
            ongoing: false,
            auto_cancel: false,
            silent: false,
            messages: Vec::new(),
            conversation_title: None,
            group_conversation: false,
            conversation_avatar_bytes: None,
            self_name: None,
            append_messages: true,
            deep_link: None,
            badge: None,
        }
    }
}

impl NotificationData {
    /// Creates a [`NotificationDataBuilder`] for constructing a notification
    /// payload without a Tauri handle.
    ///
    /// Use this where no `AppHandle` exists — most notably inside a
    /// killed-state silent-push handler registered with
    /// [`silent_push_handler!`](crate::silent_push_handler) (the iOS NSE and
    /// Android cold-start paths). For posting notifications from a running
    /// app, prefer `Notifications::builder`, which also sends them.
    #[must_use]
    pub fn builder() -> NotificationDataBuilder {
        NotificationDataBuilder {
            data: Self::default(),
        }
    }
}

/// One authoritative definition of the notification setter surface, expanded
/// into both [`NotificationDataBuilder`] (below) and `NotificationsBuilder`
/// (src/lib.rs). Both structs hold a `data: NotificationData` field; keeping
/// the setters in a macro means a new field cannot land in one builder and
/// silently miss the other.
///
/// `const`-ness note: setters that are `const` here are `const` in the
/// published `NotificationsBuilder` API — removing `const` from any of them is
/// a breaking change (`cargo semver-checks` gates this).
macro_rules! notification_setters {
    () => {
        /// Sets the notification identifier.
        #[must_use]
        pub const fn id(mut self, id: i32) -> Self {
            self.data.id = id;
            self
        }

        /// Identifier of the notification channel that delivers this
        /// notification (Android).
        ///
        /// If the channel does not exist, the notification won't fire; create
        /// it first with the plugin's channel APIs.
        #[must_use]
        pub fn channel_id(mut self, id: impl Into<String>) -> Self {
            self.data.channel_id.replace(id.into());
            self
        }

        /// Sets the notification title.
        #[must_use]
        pub fn title(mut self, title: impl Into<String>) -> Self {
            self.data.title.replace(title.into());
            self
        }

        /// Sets the notification body.
        #[must_use]
        pub fn body(mut self, body: impl Into<String>) -> Self {
            self.data.body.replace(body.into());
            self
        }

        /// Schedule this notification to fire on a later time or a fixed interval.
        #[must_use]
        pub const fn schedule(mut self, schedule: Schedule) -> Self {
            self.data.schedule.replace(schedule);
            self
        }

        /// Multiline text.
        /// Changes the notification style to big text.
        /// Cannot be used with `inboxLines`.
        #[must_use]
        pub fn large_body(mut self, large_body: impl Into<String>) -> Self {
            self.data.large_body.replace(large_body.into());
            self
        }

        /// Detail text for the notification with `largeBody`, `inboxLines` or `groupSummary`.
        #[must_use]
        pub fn summary(mut self, summary: impl Into<String>) -> Self {
            self.data.summary.replace(summary.into());
            self
        }

        /// Append a chat message, rendering the notification with Android
        /// `MessagingStyle` (per-sender circular avatars). Adding any message takes
        /// precedence over `largeBody` / `inboxLines`.
        ///
        /// On iOS this only applies to the silent-push NSE path
        /// ([`silent_push_handler!`](crate::silent_push_handler)): the last
        /// message with a sender turns the delivery into a communication
        /// notification (`INSendMessageIntent`), drawing the sender's avatar
        /// instead of the app icon. The host app must carry the
        /// `com.apple.developer.usernotifications.communication` entitlement and
        /// declare `INSendMessageIntent` in its Info.plist `NSUserActivityTypes`.
        #[must_use]
        pub fn message(mut self, message: NotificationMessage) -> Self {
            self.data.messages.push(message);
            self
        }

        /// Conversation title shown above the messages (typically the room name for a
        /// group conversation). Used with [`message`](Self::message). On iOS it
        /// becomes the communication notification's group name (with
        /// [`group_conversation`](Self::group_conversation)).
        #[must_use]
        pub fn conversation_title(mut self, title: impl Into<String>) -> Self {
            self.data.conversation_title.replace(title.into());
            self
        }

        /// Mark the conversation as a group (multiple participants), which lets the
        /// system show the conversation title.
        #[must_use]
        pub const fn group_conversation(mut self) -> Self {
            self.data.group_conversation = true;
            self
        }

        /// Set the group conversation's (room's) avatar as base64-encoded image
        /// bytes (PNG/JPEG). With
        /// [`group_conversation`](Self::group_conversation) it becomes the
        /// notification's icon in place of the sender's avatar, branding the
        /// notification as the room: the communication notification's icon on iOS
        /// (silent-push NSE path), the `MessagingStyle` conversation icon
        /// (largeIcon) on Android.
        #[must_use]
        pub fn conversation_avatar_bytes(mut self, base64: impl Into<String>) -> Self {
            self.data.conversation_avatar_bytes.replace(base64.into());
            self
        }

        /// Display name of the local user in a `MessagingStyle` conversation
        /// (defaults to "Me"). Android only.
        #[must_use]
        pub fn self_name(mut self, name: impl Into<String>) -> Self {
            self.data.self_name.replace(name.into());
            self
        }

        /// Whether a `MessagingStyle` notification appends to an already-showing
        /// notification with the same `id` (default `true`), accumulating a
        /// conversation. Set `false` to replace it instead. Android only.
        #[must_use]
        pub const fn append_messages(mut self, append: bool) -> Self {
            self.data.append_messages = append;
            self
        }

        /// Android only. Make tapping the notification open a deep link
        /// (`ACTION_VIEW` for this URI, e.g. `matrix:roomid/…`) pinned to the app's
        /// own package, instead of launching the default activity. The app's
        /// matching `<intent-filter>` receives it (e.g. via
        /// `tauri-plugin-deep-link`); this replaces the `notificationClicked`
        /// event for that tap. On iOS, put the URI in [`extra`](Self::extra)
        /// instead — string extras surface in the `notificationClicked` event's
        /// `data`.
        #[must_use]
        pub fn deep_link(mut self, uri: impl Into<String>) -> Self {
            self.data.deep_link.replace(uri.into());
            self
        }

        /// Defines an action type for this notification.
        #[must_use]
        pub fn action_type_id(mut self, action_type_id: impl Into<String>) -> Self {
            self.data.action_type_id.replace(action_type_id.into());
            self
        }

        /// Identifier used to group multiple notifications (iOS `threadIdentifier`).
        ///
        /// <https://developer.apple.com/documentation/usernotifications/unmutablenotificationcontent/1649872-threadidentifier>
        #[must_use]
        pub fn group(mut self, group: impl Into<String>) -> Self {
            self.data.group.replace(group.into());
            self
        }

        /// Instructs the system that this notification is the summary of a group on Android.
        #[must_use]
        pub const fn group_summary(mut self) -> Self {
            self.data.group_summary = true;
            self
        }

        /// The sound resource name. Only available on mobile.
        #[must_use]
        pub fn sound(mut self, sound: impl Into<String>) -> Self {
            self.data.sound.replace(sound.into());
            self
        }

        /// Append an inbox line to the notification.
        /// Changes the notification style to inbox.
        /// Cannot be used with `largeBody`.
        ///
        /// Only supports up to 5 lines.
        #[must_use]
        pub fn inbox_line(mut self, line: impl Into<String>) -> Self {
            self.data.inbox_lines.push(line.into());
            self
        }

        /// Notification icon.
        ///
        /// On Android the icon must be placed in the app's `res/drawable` folder.
        #[must_use]
        pub fn icon(mut self, icon: impl Into<String>) -> Self {
            self.data.icon.replace(icon.into());
            self
        }

        /// Notification large icon (Android).
        ///
        /// The icon must be placed in the app's `res/drawable` folder.
        #[must_use]
        pub fn large_icon(mut self, large_icon: impl Into<String>) -> Self {
            self.data.large_icon.replace(large_icon.into());
            self
        }

        /// Icon color on Android.
        #[must_use]
        pub fn icon_color(mut self, icon_color: impl Into<String>) -> Self {
            self.data.icon_color.replace(icon_color.into());
            self
        }

        /// Append an attachment to the notification.
        #[must_use]
        pub fn attachment(mut self, attachment: Attachment) -> Self {
            self.data.attachments.push(attachment);
            self
        }

        /// Adds an extra payload to store in the notification.
        #[must_use]
        pub fn extra(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
            if let Ok(value) = serde_json::to_value(value) {
                self.data.extra.insert(key.into(), value);
            }
            self
        }

        /// If true, the notification cannot be dismissed by the user on Android.
        ///
        /// An application service must manage the dismissal of the notification.
        /// It is typically used to indicate a background task that is pending (e.g. a file download)
        /// or the user is engaged with (e.g. playing music).
        #[must_use]
        pub const fn ongoing(mut self) -> Self {
            self.data.ongoing = true;
            self
        }

        /// Automatically cancel the notification when the user clicks on it.
        #[must_use]
        pub const fn auto_cancel(mut self) -> Self {
            self.data.auto_cancel = true;
            self
        }

        /// Changes the notification presentation to be silent on iOS (no badge, no sound, not listed).
        #[must_use]
        pub const fn silent(mut self) -> Self {
            self.data.silent = true;
            self
        }

        /// Sets the app icon badge count applied with this notification (iOS
        /// silent-push NSE path only; ignored elsewhere). Omit to leave the
        /// current badge unchanged.
        #[must_use]
        pub const fn badge(mut self, count: i32) -> Self {
            self.data.badge.replace(count);
            self
        }
    };
}
pub(crate) use notification_setters;

/// Builds a [`NotificationData`] without requiring a Tauri handle.
///
/// Same setter surface as `NotificationsBuilder` (both expand
/// `notification_setters!`), but [`build`] returns the payload instead of
/// sending it — the caller (e.g. the plugin's iOS Notification Service
/// Extension) is responsible for displaying it.
///
/// [`build`]: Self::build
#[derive(Debug)]
pub struct NotificationDataBuilder {
    data: NotificationData,
}

impl NotificationDataBuilder {
    notification_setters!();

    /// Returns the built payload.
    #[must_use]
    pub fn build(self) -> NotificationData {
        self.data
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingNotification {
    pub(crate) id: i32,
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) schedule: Schedule,
}

impl PendingNotification {
    #[must_use]
    pub const fn id(&self) -> i32 {
        self.id
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    #[must_use]
    pub const fn schedule(&self) -> &Schedule {
        &self.schedule
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveNotification {
    pub(crate) id: i32,
    pub(crate) tag: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(default)]
    pub(crate) group_summary: bool,
    #[serde(default)]
    pub(crate) data: HashMap<String, String>,
    #[serde(default)]
    pub(crate) extra: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) action_type_id: Option<String>,
    pub(crate) schedule: Option<Schedule>,
    pub(crate) sound: Option<String>,
}

impl ActiveNotification {
    /// Constructs an `ActiveNotification` from the minimal set of fields the
    /// desktop tracking layer can populate. All other fields fall back to
    /// their defaults (the mobile/native paths fill them out fully).
    #[must_use]
    pub fn new(id: i32, title: Option<String>, body: Option<String>) -> Self {
        Self {
            id,
            tag: None,
            title,
            body,
            group: None,
            group_summary: false,
            data: HashMap::new(),
            extra: HashMap::new(),
            attachments: Vec::new(),
            action_type_id: None,
            schedule: None,
            sound: None,
        }
    }

    #[must_use]
    pub const fn id(&self) -> i32 {
        self.id
    }

    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    #[must_use]
    pub fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }

    #[must_use]
    pub const fn group_summary(&self) -> bool {
        self.group_summary
    }

    #[must_use]
    pub const fn data(&self) -> &HashMap<String, String> {
        &self.data
    }

    #[must_use]
    pub const fn extra(&self) -> &HashMap<String, serde_json::Value> {
        &self.extra
    }

    #[must_use]
    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    #[must_use]
    pub fn action_type_id(&self) -> Option<&str> {
        self.action_type_id.as_deref()
    }

    #[must_use]
    pub const fn schedule(&self) -> Option<&Schedule> {
        self.schedule.as_ref()
    }

    #[must_use]
    pub fn sound(&self) -> Option<&str> {
        self.sound.as_deref()
    }
}

// Each bool is an independent UNNotificationCategory option; grouping would change the JSON shape.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionType {
    id: String,
    actions: Vec<Action>,
    hidden_previews_body_placeholder: Option<String>,
    #[serde(default)]
    custom_dismiss_action: bool,
    #[serde(default)]
    allow_in_car_play: bool,
    #[serde(default)]
    hidden_previews_show_title: bool,
    #[serde(default)]
    hidden_previews_show_subtitle: bool,
}

impl ActionType {
    pub fn new(id: impl Into<String>, actions: Vec<Action>) -> Self {
        Self {
            id: id.into(),
            actions,
            hidden_previews_body_placeholder: None,
            custom_dismiss_action: false,
            allow_in_car_play: false,
            hidden_previews_show_title: false,
            hidden_previews_show_subtitle: false,
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }
}

// Each bool is an independent UNNotificationAction option; grouping would change the JSON shape.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    id: String,
    title: String,
    #[serde(default)]
    requires_authentication: bool,
    #[serde(default)]
    foreground: bool,
    #[serde(default)]
    destructive: bool,
    #[serde(default)]
    input: bool,
    input_button_title: Option<String>,
    input_placeholder: Option<String>,
}

impl Action {
    pub fn new(id: impl Into<String>, title: impl Into<String>, foreground: bool) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            requires_authentication: false,
            foreground,
            destructive: false,
            input: false,
            input_button_title: None,
            input_placeholder: None,
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn foreground(&self) -> bool {
        self.foreground
    }
}

pub use android::*;

mod android {
    use serde::{Deserialize, Serialize};
    use serde_repr::{Deserialize_repr, Serialize_repr};

    #[derive(Debug, Default, Clone, Copy, Serialize_repr, Deserialize_repr)]
    #[repr(u8)]
    pub enum Importance {
        None = 0,
        Min = 1,
        Low = 2,
        #[default]
        Default = 3,
        High = 4,
    }

    #[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
    #[repr(i8)]
    pub enum Visibility {
        Secret = -1,
        Private = 0,
        Public = 1,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Channel {
        id: String,
        name: String,
        description: Option<String>,
        sound: Option<String>,
        lights: Option<bool>,
        light_color: Option<String>,
        vibration: Option<bool>,
        importance: Option<Importance>,
        visibility: Option<Visibility>,
    }

    #[derive(Debug)]
    pub struct ChannelBuilder(Channel);

    impl Channel {
        pub fn builder(id: impl Into<String>, name: impl Into<String>) -> ChannelBuilder {
            ChannelBuilder(Self {
                id: id.into(),
                name: name.into(),
                description: None,
                sound: None,
                lights: Some(false),
                light_color: None,
                vibration: Some(false),
                importance: None,
                visibility: None,
            })
        }

        #[must_use]
        pub fn id(&self) -> &str {
            &self.id
        }

        #[must_use]
        pub fn name(&self) -> &str {
            &self.name
        }

        #[must_use]
        pub fn description(&self) -> Option<&str> {
            self.description.as_deref()
        }

        #[must_use]
        pub fn sound(&self) -> Option<&str> {
            self.sound.as_deref()
        }

        #[must_use]
        pub fn lights(&self) -> bool {
            self.lights.unwrap_or(false)
        }

        #[must_use]
        pub fn light_color(&self) -> Option<&str> {
            self.light_color.as_deref()
        }

        #[must_use]
        pub fn vibration(&self) -> bool {
            self.vibration.unwrap_or(false)
        }

        #[must_use]
        pub fn importance(&self) -> Importance {
            self.importance.unwrap_or_default()
        }

        #[must_use]
        pub const fn visibility(&self) -> Option<Visibility> {
            self.visibility
        }
    }

    impl ChannelBuilder {
        #[must_use]
        pub fn description(mut self, description: impl Into<String>) -> Self {
            self.0.description.replace(description.into());
            self
        }

        #[must_use]
        pub fn sound(mut self, sound: impl Into<String>) -> Self {
            self.0.sound.replace(sound.into());
            self
        }

        #[must_use]
        pub const fn lights(mut self, lights: bool) -> Self {
            self.0.lights = Some(lights);
            self
        }

        #[must_use]
        pub fn light_color(mut self, color: impl Into<String>) -> Self {
            self.0.light_color.replace(color.into());
            self
        }

        #[must_use]
        pub const fn vibration(mut self, vibration: bool) -> Self {
            self.0.vibration = Some(vibration);
            self
        }

        #[must_use]
        pub const fn importance(mut self, importance: Importance) -> Self {
            self.0.importance = Some(importance);
            self
        }

        #[must_use]
        pub const fn visibility(mut self, visibility: Visibility) -> Self {
            self.0.visibility.replace(visibility);
            self
        }

        #[must_use]
        pub fn build(self) -> Channel {
            self.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attachment_creation() {
        let url = Url::parse("https://example.com/image.png").expect("Failed to parse URL");
        let attachment = Attachment::new("test_id", url.clone());
        assert_eq!(attachment.id, "test_id");
        assert_eq!(attachment.url, url);
    }

    #[test]
    fn test_attachment_serialization() {
        let url = Url::parse("https://example.com/image.png").expect("Failed to parse URL");
        let attachment = Attachment::new("test_id", url);
        let json = serde_json::to_string(&attachment).expect("Failed to serialize attachment");
        assert!(json.contains("test_id"));
        assert!(json.contains("https://example.com/image.png"));
    }

    #[test]
    fn test_attachment_deserialization() {
        let json = r#"{"id":"test_id","url":"https://example.com/image.png"}"#;
        let attachment: Attachment =
            serde_json::from_str(json).expect("Failed to deserialize attachment");
        assert_eq!(attachment.id, "test_id");
        assert_eq!(attachment.url.as_str(), "https://example.com/image.png");
    }

    #[test]
    fn test_notification_data_builder_camel_case_json() {
        // The iOS Notification Service Extension decodes exactly this JSON
        // (see ios/NSE/Sources/SilentPushContent.swift) — field names are the
        // camelCase serde contract.
        let url = Url::parse("file:///attachment.png").expect("Failed to parse URL");
        let data = NotificationData::builder()
            .id(7)
            .title("Alice")
            .body("hello")
            .summary("!room:matrix.org")
            .group("!room:matrix.org")
            .action_type_id("message")
            .sound("ping.caf")
            .attachment(Attachment::new("a1", url))
            .extra("deepLink", "matrix:roomid/room:matrix.org/e/xyz")
            .extra("count", 2)
            .build();

        let json = serde_json::to_value(&data).expect("Failed to serialize NotificationData");
        assert_eq!(json["id"], 7);
        assert_eq!(json["title"], "Alice");
        assert_eq!(json["body"], "hello");
        assert_eq!(json["summary"], "!room:matrix.org");
        assert_eq!(json["group"], "!room:matrix.org");
        assert_eq!(json["actionTypeId"], "message");
        assert_eq!(json["sound"], "ping.caf");
        assert_eq!(json["attachments"][0]["id"], "a1");
        assert_eq!(json["attachments"][0]["url"], "file:///attachment.png");
        assert_eq!(
            json["extra"]["deepLink"],
            "matrix:roomid/room:matrix.org/e/xyz"
        );
        assert_eq!(json["extra"]["count"], 2);
    }

    #[test]
    fn test_notification_data_builder_round_trip() {
        let data = NotificationData::builder()
            .title("t")
            .group("g")
            .extra("k", "v")
            .build();
        let json = serde_json::to_string(&data).expect("Failed to serialize");
        let back: NotificationData = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(back.title.as_deref(), Some("t"));
        assert_eq!(back.group.as_deref(), Some("g"));
        assert_eq!(back.extra.get("k"), Some(&serde_json::json!("v")));
    }

    #[test]
    fn test_schedule_every_display() {
        assert_eq!(ScheduleEvery::Year.to_string(), "year");
        assert_eq!(ScheduleEvery::Month.to_string(), "month");
        assert_eq!(ScheduleEvery::TwoWeeks.to_string(), "twoWeeks");
        assert_eq!(ScheduleEvery::Week.to_string(), "week");
        assert_eq!(ScheduleEvery::Day.to_string(), "day");
        assert_eq!(ScheduleEvery::Hour.to_string(), "hour");
        assert_eq!(ScheduleEvery::Minute.to_string(), "minute");
        assert_eq!(ScheduleEvery::Second.to_string(), "second");
    }

    #[test]
    fn test_schedule_every_serialization() {
        let json = serde_json::to_string(&ScheduleEvery::Day).expect("Failed to serialize Day");
        assert_eq!(json, "\"day\"");

        let json =
            serde_json::to_string(&ScheduleEvery::TwoWeeks).expect("Failed to serialize TwoWeeks");
        assert_eq!(json, "\"twoWeeks\"");
    }

    #[test]
    fn test_schedule_every_deserialization() {
        let every: ScheduleEvery =
            serde_json::from_str("\"year\"").expect("Failed to deserialize year");
        assert!(matches!(every, ScheduleEvery::Year));

        let every: ScheduleEvery =
            serde_json::from_str("\"month\"").expect("Failed to deserialize month");
        assert!(matches!(every, ScheduleEvery::Month));

        let every: ScheduleEvery =
            serde_json::from_str("\"twoweeks\"").expect("Failed to deserialize twoweeks");
        assert!(matches!(every, ScheduleEvery::TwoWeeks));

        let every: ScheduleEvery =
            serde_json::from_str("\"week\"").expect("Failed to deserialize week");
        assert!(matches!(every, ScheduleEvery::Week));

        let every: ScheduleEvery =
            serde_json::from_str("\"day\"").expect("Failed to deserialize day");
        assert!(matches!(every, ScheduleEvery::Day));

        let every: ScheduleEvery =
            serde_json::from_str("\"hour\"").expect("Failed to deserialize hour");
        assert!(matches!(every, ScheduleEvery::Hour));

        let every: ScheduleEvery =
            serde_json::from_str("\"minute\"").expect("Failed to deserialize minute");
        assert!(matches!(every, ScheduleEvery::Minute));

        let every: ScheduleEvery =
            serde_json::from_str("\"second\"").expect("Failed to deserialize second");
        assert!(matches!(every, ScheduleEvery::Second));
    }

    #[test]
    fn test_schedule_every_deserialization_invalid() {
        let result: Result<ScheduleEvery, _> = serde_json::from_str("\"invalid\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_schedule_interval_default() {
        let interval = ScheduleInterval::default();
        assert!(interval.year.is_none());
        assert!(interval.month.is_none());
        assert!(interval.day.is_none());
        assert!(interval.weekday.is_none());
        assert!(interval.hour.is_none());
        assert!(interval.minute.is_none());
        assert!(interval.second.is_none());
    }

    #[test]
    fn test_schedule_interval_serialization() {
        let interval = ScheduleInterval {
            year: Some(24),
            month: Some(12),
            day: Some(25),
            weekday: Some(1),
            hour: Some(10),
            minute: Some(30),
            second: Some(0),
        };
        let json = serde_json::to_string(&interval).expect("Failed to serialize interval");
        assert!(json.contains("\"year\":24"));
        assert!(json.contains("\"month\":12"));
        assert!(json.contains("\"day\":25"));
    }

    #[test]
    fn test_notification_data_default() {
        let data = NotificationData::default();
        assert!(data.id != 0); // Should be a random ID
        assert!(data.channel_id.is_none());
        assert!(data.title.is_none());
        assert!(data.body.is_none());
        assert!(data.schedule.is_none());
        assert!(!data.group_summary);
        assert!(!data.ongoing);
        assert!(!data.auto_cancel);
        assert!(!data.silent);
        assert!(data.inbox_lines.is_empty());
        assert!(data.attachments.is_empty());
        assert!(data.extra.is_empty());
    }

    #[test]
    fn test_notification_data_serialization() {
        let data = NotificationData {
            id: 123,
            title: Some("Test Title".to_string()),
            body: Some("Test Body".to_string()),
            ongoing: true,
            ..Default::default()
        };

        let json = serde_json::to_string(&data).expect("Failed to serialize notification data");
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"title\":\"Test Title\""));
        assert!(json.contains("\"body\":\"Test Body\""));
        assert!(json.contains("\"ongoing\":true"));
    }

    #[test]
    fn test_pending_notification_getters() {
        let json = r#"{
            "id": 456,
            "title": "Pending Title",
            "body": "Pending Body",
            "schedule": {"every": {"interval": "day", "count": 1}}
        }"#;
        let pending: PendingNotification =
            serde_json::from_str(json).expect("Failed to deserialize pending notification");

        assert_eq!(pending.id(), 456);
        assert_eq!(pending.title(), Some("Pending Title"));
        assert_eq!(pending.body(), Some("Pending Body"));
        assert!(matches!(pending.schedule(), Schedule::Every { .. }));
    }

    #[test]
    fn test_active_notification_getters() {
        let json = r#"{
            "id": 789,
            "title": "Active Title",
            "body": "Active Body",
            "group": "test_group",
            "groupSummary": true
        }"#;
        let active: ActiveNotification =
            serde_json::from_str(json).expect("Failed to deserialize active notification");

        assert_eq!(active.id(), 789);
        assert_eq!(active.title(), Some("Active Title"));
        assert_eq!(active.body(), Some("Active Body"));
        assert_eq!(active.group(), Some("test_group"));
        assert!(active.group_summary());
        assert!(active.data().is_empty());
        assert!(active.extra().is_empty());
        assert!(active.attachments().is_empty());
        assert!(active.action_type_id().is_none());
        assert!(active.schedule().is_none());
        assert!(active.sound().is_none());
    }

    #[cfg(target_os = "android")]
    #[test]
    fn test_importance_default() {
        let importance = Importance::default();
        assert!(matches!(importance, Importance::Default));
    }

    #[cfg(target_os = "android")]
    #[test]
    fn test_importance_serialization() {
        assert_eq!(
            serde_json::to_string(&Importance::None).expect("Failed to serialize Importance::None"),
            "0"
        );
        assert_eq!(
            serde_json::to_string(&Importance::Min).expect("Failed to serialize Importance::Min"),
            "1"
        );
        assert_eq!(
            serde_json::to_string(&Importance::Low).expect("Failed to serialize Importance::Low"),
            "2"
        );
        assert_eq!(
            serde_json::to_string(&Importance::Default)
                .expect("Failed to serialize Importance::Default"),
            "3"
        );
        assert_eq!(
            serde_json::to_string(&Importance::High).expect("Failed to serialize Importance::High"),
            "4"
        );
    }

    #[cfg(target_os = "android")]
    #[test]
    fn test_visibility_serialization() {
        assert_eq!(
            serde_json::to_string(&Visibility::Secret)
                .expect("Failed to serialize Visibility::Secret"),
            "-1"
        );
        assert_eq!(
            serde_json::to_string(&Visibility::Private)
                .expect("Failed to serialize Visibility::Private"),
            "0"
        );
        assert_eq!(
            serde_json::to_string(&Visibility::Public)
                .expect("Failed to serialize Visibility::Public"),
            "1"
        );
    }

    #[cfg(target_os = "android")]
    #[test]
    fn test_channel_builder() {
        let channel = Channel::builder("test_id", "Test Channel")
            .description("Test Description")
            .sound("test_sound")
            .lights(true)
            .light_color("#FF0000")
            .vibration(true)
            .importance(Importance::High)
            .visibility(Visibility::Public)
            .build();

        assert_eq!(channel.id(), "test_id");
        assert_eq!(channel.name(), "Test Channel");
        assert_eq!(channel.description(), Some("Test Description"));
        assert_eq!(channel.sound(), Some("test_sound"));
        assert!(channel.lights());
        assert_eq!(channel.light_color(), Some("#FF0000"));
        assert!(channel.vibration());
        assert!(matches!(channel.importance(), Importance::High));
        assert_eq!(channel.visibility(), Some(Visibility::Public));
    }

    #[cfg(target_os = "android")]
    #[test]
    fn test_channel_builder_minimal() {
        let channel = Channel::builder("minimal_id", "Minimal Channel").build();

        assert_eq!(channel.id(), "minimal_id");
        assert_eq!(channel.name(), "Minimal Channel");
        assert_eq!(channel.description(), None);
        assert_eq!(channel.sound(), None);
        assert!(!channel.lights());
        assert_eq!(channel.light_color(), None);
        assert!(!channel.vibration());
        assert!(matches!(channel.importance(), Importance::Default));
        assert_eq!(channel.visibility(), None);
    }

    #[test]
    fn test_schedule_at_serialization() {
        use time::OffsetDateTime;

        let date = OffsetDateTime::now_utc();
        let schedule = Schedule::At {
            date,
            repeating: true,
            allow_while_idle: false,
        };

        let json = serde_json::to_string(&schedule).expect("Failed to serialize Schedule::At");
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"date\""));
        assert!(json.contains("\"repeating\":true"));
        assert!(json.contains("\"allowWhileIdle\":false"));
    }

    #[test]
    fn test_schedule_interval_variant() {
        let schedule = Schedule::Interval {
            interval: ScheduleInterval {
                hour: Some(10),
                minute: Some(30),
                ..Default::default()
            },
            allow_while_idle: true,
        };

        let json =
            serde_json::to_string(&schedule).expect("Failed to serialize Schedule::Interval");
        assert!(json.contains("\"interval\""));
        assert!(json.contains("\"hour\":10"));
        assert!(json.contains("\"minute\":30"));
        assert!(json.contains("\"allowWhileIdle\":true"));
    }

    #[test]
    fn test_schedule_every_variant() {
        let schedule = Schedule::Every {
            interval: ScheduleEvery::Day,
            count: 5,
            allow_while_idle: false,
        };

        let json = serde_json::to_string(&schedule).expect("Failed to serialize Schedule::Every");
        assert!(json.contains("\"every\""));
        assert!(json.contains("\"interval\":\"day\""));
        assert!(json.contains("\"count\":5"));
    }
}
