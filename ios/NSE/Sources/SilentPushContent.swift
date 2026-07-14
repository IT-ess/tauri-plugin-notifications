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
  /// Merged into the notification's `userInfo` at the top level (APNs
  /// convention), on top of the push's own custom keys.
  let extra: [String: JSONValue]?
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
  /// Marks the conversation as a group (multiple participants).
  let groupConversation: Bool?

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
    if let extra = extra {
      var userInfo = content.userInfo
      for (key, value) in extra {
        if let string = value.stringified {
          userInfo[key] = string
        }
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
  /// `INSendMessageIntent` for the sender and returns `content.updating(from:)`,
  /// which makes the system draw the sender's avatar instead of the app icon.
  /// Falls back to `content` unchanged if the rewrite fails (e.g. the host app
  /// lacks the communication-notifications entitlement).
  @available(iOS 15.0, macOS 12.0, *)
  private func communicationContent(
    from content: UNMutableNotificationContent,
    message: SilentPushMessage,
    sender: String,
    log: OSLog
  ) -> UNNotificationContent {
    var avatar: INImage?
    if let base64 = message.avatarBytes, let data = Data(base64Encoded: base64) {
      avatar = INImage(imageData: data)
    }
    let senderKey = message.personKey ?? sender
    let person = INPerson(
      personHandle: INPersonHandle(value: senderKey, type: .unknown),
      nameComponents: nil,
      displayName: sender,
      image: avatar,
      contactIdentifier: nil,
      customIdentifier: senderKey
    )

    let isGroup = groupConversation ?? false
    let intent = INSendMessageIntent(
      recipients: nil,
      outgoingMessageType: .outgoingMessageText,
      content: message.text ?? body,
      speakableGroupName: isGroup
        ? conversationTitle.map { INSpeakableString(spokenPhrase: $0) } : nil,
      conversationIdentifier: group,
      serviceName: nil,
      sender: person,
      attachments: nil
    )

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

/// An arbitrary JSON value, as `extra` values can be any JSON. Everything is
/// stringified before landing in `userInfo` so the values survive the plugin's
/// `notificationClicked` plumbing, which forwards string values only.
enum JSONValue: Decodable {
  case string(String)
  case number(Double)
  case bool(Bool)
  case array([JSONValue])
  case object([String: JSONValue])
  case null

  init(from decoder: Decoder) throws {
    let container = try decoder.singleValueContainer()
    if container.decodeNil() {
      self = .null
    } else if let bool = try? container.decode(Bool.self) {
      self = .bool(bool)
    } else if let number = try? container.decode(Double.self) {
      self = .number(number)
    } else if let string = try? container.decode(String.self) {
      self = .string(string)
    } else if let array = try? container.decode([JSONValue].self) {
      self = .array(array)
    } else if let object = try? container.decode([String: JSONValue].self) {
      self = .object(object)
    } else {
      throw DecodingError.dataCorrupted(
        DecodingError.Context(
          codingPath: decoder.codingPath, debugDescription: "Unsupported JSON value"))
    }
  }

  /// String form used for `userInfo`; `nil` for JSON null (the entry is skipped).
  var stringified: String? {
    switch self {
    case .string(let string):
      return string
    case .number(let number):
      // Render integral numbers without a trailing ".0", like serde_json does.
      if number.truncatingRemainder(dividingBy: 1) == 0, abs(number) < 1e15 {
        return String(Int64(number))
      }
      return String(number)
    case .bool(let bool):
      return bool ? "true" : "false"
    case .array, .object:
      guard let data = try? JSONEncoder().encode(self),
        let string = String(data: data, encoding: .utf8)
      else {
        return nil
      }
      return string
    case .null:
      return nil
    }
  }
}

extension JSONValue: Encodable {
  func encode(to encoder: Encoder) throws {
    var container = encoder.singleValueContainer()
    switch self {
    case .string(let string): try container.encode(string)
    case .number(let number): try container.encode(number)
    case .bool(let bool): try container.encode(bool)
    case .array(let array): try container.encode(array)
    case .object(let object): try container.encode(object)
    case .null: try container.encodeNil()
    }
  }
}
