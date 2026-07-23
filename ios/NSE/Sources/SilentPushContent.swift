import Foundation
import Intents
import UserNotifications
import os.log

/// Notification content returned by the host app's Rust silent-push handler —
/// the JSON form of the plugin's `NotificationData` (see `src/models.rs`,
/// camelCase field names).
///
/// Only the fields an iOS Notification Service Extension can apply are
/// consumed; unknown or Android-only fields (`channelId`, `icon`,
/// `inboxLines`, …) are legal in the JSON and ignored here. `id` is also
/// ignored: an NSE cannot change the request identifier APNs assigned, so
/// remote notifications surface in the plugin's `notificationClicked` event
/// with `id: -1` — put routing information (e.g. a `deepLink`) in `extra`
/// instead, whose string values reach that event's `data`.
struct SilentPushContent: Decodable {
  let title: String?
  let body: String?
  /// Values of the handler's `extra` map, stringified; merged into the
  /// notification's `userInfo` at the top level (APNs convention), on top of
  /// the push's own custom keys. Deliberately not Codable-decoded: the service
  /// fills it from the raw result JSON via `JSONSerialization`, whose NSNumber
  /// preserves the exact text of large integers where a Double round-trip
  /// would corrupt anything ≥ 2^53.
  var extraStrings: [String: String] = [:]
  /// Sound resource name, applied as `UNNotificationSound(named:)`.
  let sound: String?
  /// Grouping key, applied as `threadIdentifier`.
  let group: String?
  /// Applied as `summaryArgument`.
  let summary: String?
  /// Category identifier (action type registered by the app).
  let actionTypeId: String?
  /// Attachments; URLs must be file URLs readable from the extension's
  /// sandbox (i.e. inside the shared App Group container).
  let attachments: [SilentPushAttachment]?
  /// Chat messages — the Android `MessagingStyle` fields. On iOS the *last*
  /// message turns the notification into a communication notification
  /// (`INSendMessageIntent`): the sender's avatar replaces the app icon,
  /// like Android's per-sender circular avatars. Requires the host app to
  /// carry the `com.apple.developer.usernotifications.communication`
  /// entitlement and declare `INSendMessageIntent` in `NSUserActivityTypes`.
  let messages: [SilentPushMessage]?
  /// Conversation title (room name); shown as the group name when
  /// `groupConversation` is set.
  let conversationTitle: String?
  /// Marks the conversation as a group (multiple participants): the
  /// notification then brands as the room — `conversationTitle` as the group
  /// name and `conversationAvatarBytes` as the icon — instead of the sender.
  let groupConversation: Bool?
  /// Avatar of the group conversation (room) as base64-encoded image bytes;
  /// with `groupConversation`, drawn as the notification icon instead of the
  /// sender's avatar.
  let conversationAvatarBytes: String?
  /// App icon badge count; `nil` leaves the current badge unchanged.
  let badge: Int?

  private enum CodingKeys: String, CodingKey {
    case title, body, sound, group, summary, actionTypeId, attachments,
      messages, conversationTitle, groupConversation, conversationAvatarBytes,
      badge
  }

  /// Applies the decoded fields onto `content` (the mutable copy of the push's
  /// original content), leaving everything the handler omitted — most notably
  /// the payload's fallback `alert` — untouched. Returns the content to
  /// deliver: `content` itself, or the communication-notification rewrite of
  /// it when `messages` carries a sender.
  func apply(to content: UNMutableNotificationContent, log: OSLog) -> UNNotificationContent {
    if let title = title {
      content.title = title
    }
    if let body = body {
      content.body = body
    }
    if !extraStrings.isEmpty {
      var userInfo = content.userInfo
      for (key, value) in extraStrings {
        userInfo[key] = value
      }
      content.userInfo = userInfo
    }
    if let group = group {
      content.threadIdentifier = group
    }
    if let summary = summary {
      content.summaryArgument = summary
    }
    if let actionTypeId = actionTypeId {
      content.categoryIdentifier = actionTypeId
    }
    if let sound = sound {
      content.sound = UNNotificationSound(named: UNNotificationSoundName(sound))
    }
    // Only set when the count is known: a nil badge leaves the app icon's
    // current badge untouched (never guess a number).
    if let badge = badge {
      content.badge = NSNumber(value: badge)
    }
    if let attachments = attachments, !attachments.isEmpty {
      var created: [UNNotificationAttachment] = []
      for attachment in attachments {
        guard let url = URL(string: attachment.url) else {
          os_log(
            "Skipping attachment %{public}@: invalid URL", log: log, type: .error, attachment.id)
          continue
        }
        do {
          created.append(try UNNotificationAttachment(identifier: attachment.id, url: url))
        } catch {
          os_log(
            "Skipping attachment %{public}@: %{public}@", log: log, type: .error, attachment.id,
            error.localizedDescription)
        }
      }
      if !created.isEmpty {
        content.attachments = created
      }
    }

    // Communication notification: iOS's counterpart of Android's
    // MessagingStyle. Only the most recent message matters — iOS stacks
    // earlier ones via `threadIdentifier` — and it needs a sender to render.
    if #available(iOS 15.0, macOS 12.0, *),
      let message = messages?.last,
      let sender = message.sender
    {
      return communicationContent(from: content, message: message, sender: sender, log: log)
    }
    return content
  }

  /// Rewrites `content` as a communication notification: donates an incoming
  /// `INSendMessageIntent` for the sender and returns `content.updating(from:)`.
  /// DMs draw the sender's avatar instead of the app icon; group conversations
  /// brand as the room instead — `conversationTitle` becomes the displayed
  /// group name and `conversationAvatarBytes` the icon. Falls back to `content`
  /// unchanged if the rewrite fails (e.g. the host app lacks the
  /// communication-notifications entitlement).
  @available(iOS 15.0, macOS 12.0, *)
  private func communicationContent(
    from content: UNMutableNotificationContent,
    message: SilentPushMessage,
    sender: String,
    log: OSLog
  ) -> UNNotificationContent {
    let isGroup = groupConversation ?? false

    // Group conversations brand as the room: the room's avatar (or, without
    // one, the system's monogram of the group name) is the icon, so the sender
    // person deliberately carries no image there — a sender image would
    // compete for the icon slot. DMs keep the sender's avatar.
    var groupAvatar: INImage?
    if isGroup, let base64 = conversationAvatarBytes, let data = Data(base64Encoded: base64) {
      groupAvatar = INImage(imageData: data)
    }
    var senderAvatar: INImage?
    if !isGroup, let base64 = message.avatarBytes, let data = Data(base64Encoded: base64) {
      senderAvatar = INImage(imageData: data)
    }

    let senderKey = message.personKey ?? sender
    let person = INPerson(
      personHandle: INPersonHandle(value: senderKey, type: .unknown),
      nameComponents: nil,
      displayName: sender,
      image: senderAvatar,
      contactIdentifier: nil,
      customIdentifier: senderKey
    )

    // The system only treats the intent as a group conversation — rendering
    // `speakableGroupName` and its image — when it has *multiple* recipients;
    // a lone `isMe` person doesn't qualify and the notification falls back to
    // 1:1 styling. The room's member list isn't available here, so mirror
    // Element X: the sender plus an `isMe` placeholder for the local user.
    var recipients: [INPerson]?
    if isGroup {
      recipients = [
        person,
        INPerson(
          personHandle: INPersonHandle(value: "me", type: .unknown),
          nameComponents: nil,
          displayName: nil,
          image: nil,
          contactIdentifier: nil,
          customIdentifier: nil,
          isMe: true
        ),
      ]
    }

    let intent = INSendMessageIntent(
      recipients: recipients,
      outgoingMessageType: .outgoingMessageText,
      content: message.text ?? body,
      speakableGroupName: isGroup
        ? conversationTitle.map { INSpeakableString(spokenPhrase: $0) } : nil,
      conversationIdentifier: group,
      serviceName: nil,
      sender: person,
      attachments: nil
    )
    // `setImage(_:forParameterNamed:)` doesn't exist on macOS; there the group
    // notification keeps the monogram the system derives from the group name.
    #if !os(macOS)
      if let groupAvatar = groupAvatar {
        intent.setImage(groupAvatar, forParameterNamed: \.speakableGroupName)
      }
    #endif

    let interaction = INInteraction(intent: intent, response: nil)
    interaction.direction = .incoming
    interaction.donate(completion: nil)

    do {
      return try content.updating(from: intent)
    } catch {
      os_log(
        "Failed to rewrite as communication notification (missing entitlement?): %{public}@",
        log: log, type: .error, String(describing: error))
      return content
    }
  }
}

/// One entry of `SilentPushContent.messages` — the serialized form of the Rust
/// `NotificationMessage` model (shared with Android's `MessagingStyle` path).
struct SilentPushMessage: Decodable {
  /// Sender display name; without it the message cannot become a
  /// communication notification.
  let sender: String?
  /// Stable sender key (e.g. a Matrix user id), used to merge senders.
  let personKey: String?
  /// Sender avatar as base64-encoded image bytes (PNG/JPEG).
  let avatarBytes: String?
  /// Message text; falls back to the notification `body`.
  let text: String?
}

/// One entry of `SilentPushContent.attachments` — the serialized form of the
/// Rust `Attachment` model.
struct SilentPushAttachment: Decodable {
  let id: String
  let url: String
}
