import Foundation
import UserNotifications
import os.log

/// Notification content returned by the host app's Rust silent-push handler —
/// the JSON form of the plugin's `NotificationData` (see `src/models.rs`,
/// camelCase field names).
///
/// Only the fields an iOS Notification Service Extension can apply are
/// consumed; unknown or Android-only fields (`messages`, `channelId`, `icon`,
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

  /// Applies the decoded fields onto `content` (the mutable copy of the push's
  /// original content), leaving everything the handler omitted — most notably
  /// the payload's fallback `alert` — untouched.
  func apply(to content: UNMutableNotificationContent, log: OSLog) {
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
  }
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
