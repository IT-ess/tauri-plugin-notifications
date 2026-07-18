import Foundation
import UserNotifications
import os.log

/// Base class for the host app's Notification Service Extension, mirroring the
/// Android killed-state path (the plugin's messaging service + JNI): the push
/// payload's custom keys are handed to Rust code in the host app's own static
/// library, which fetches/decodes the real content (e.g. a Matrix event) and
/// returns the notification to display.
///
/// Usage — in the app's NSE target (which links the same `libapp.a` staticlib
/// as the app, plus this module):
///
/// ```swift
/// import TauriPluginNotificationsNSE
///
/// final class NotificationService: TauriNotificationService {}
/// ```
///
/// The Rust side is registered with the plugin's `silent_push_handler!`
/// macro. Because `libapp.a` is a static archive, the NSE target must force
/// the symbols in with
/// `OTHER_LDFLAGS: -Wl,-u,_tauri_notifications_process_silent_push -Wl,-u,_tauri_notifications_silent_push_free`
/// or `dlsym` finds nothing (the object file would be dead-stripped).
///
/// The extension's Info.plist should carry the shared App Group id under
/// `TauriNotificationsAppGroup`; its container path is passed to the Rust
/// handler as `data_dir`, so it can open the store the main app shares there.
///
/// Failure semantics: whenever anything goes wrong — missing symbols, the
/// handler returning `None` or panicking, malformed JSON, or the ~30 s system
/// deadline expiring — the push's original content is delivered unchanged, so
/// the payload's fallback `alert` (e.g. `loc-key: SINGLE_UNREAD`) still shows.
/// A notification is never dropped.
open class TauriNotificationService: UNNotificationServiceExtension {
  private static let log = OSLog(
    subsystem: "app.tauri.notifications", category: "TauriNotificationService")

  private typealias ProcessFn = @convention(c) (
    UnsafePointer<CChar>?, UnsafePointer<CChar>?
  ) -> UnsafeMutablePointer<CChar>?
  private typealias FreeFn = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void

  // Both guarded by `deliveryLock`; `deliver` atomically takes `contentHandler`
  // (swapping in nil), which is what makes delivery happen exactly once.
  private var contentHandler: ((UNNotificationContent) -> Void)?
  private var bestAttemptContent: UNMutableNotificationContent?
  private let deliveryLock = NSLock()

  /// Info.plist key holding the App Group identifier. Override to rename.
  open var appGroupInfoPlistKey: String { "TauriNotificationsAppGroup" }

  /// Resolves the directory passed to the Rust handler as `data_dir`.
  ///
  /// Default: the container of the App Group named in the extension's
  /// Info.plist under `appGroupInfoPlistKey`. Falls back to the extension's
  /// own home directory (which the main app cannot see) with an os_log fault
  /// when the key or container is missing. Override for custom resolution.
  open func dataDirectory() -> String {
    guard
      let group = Bundle.main.object(forInfoDictionaryKey: appGroupInfoPlistKey) as? String
    else {
      os_log(
        "No %{public}@ key in the extension's Info.plist; passing the extension's own home directory as data_dir",
        log: Self.log, type: .fault, appGroupInfoPlistKey)
      return NSHomeDirectory()
    }
    guard
      let container = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: group)
    else {
      os_log(
        "App Group %{public}@ container unavailable (entitlement missing?); passing the extension's own home directory as data_dir",
        log: Self.log, type: .fault, group)
      return NSHomeDirectory()
    }
    return container.path
  }

  override open func didReceive(
    _ request: UNNotificationRequest,
    withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
  ) {
    deliveryLock.lock()
    self.contentHandler = contentHandler
    self.bestAttemptContent = request.content.mutableCopy() as? UNMutableNotificationContent
    deliveryLock.unlock()

    let userInfo = request.content.userInfo
    DispatchQueue.global(qos: .userInitiated).async { [weak self] in
      self?.processPush(userInfo)
    }
  }

  override open func serviceExtensionTimeWillExpire() {
    // Out of time: show the payload's own (fallback) content.
    deliver(nil)
  }

  private func processPush(_ userInfo: [AnyHashable: Any]) {
    guard
      let process = symbol("tauri_notifications_process_silent_push", as: ProcessFn.self),
      let free = symbol("tauri_notifications_silent_push_free", as: FreeFn.self)
    else {
      os_log(
        "Rust silent-push symbols not found. Did you invoke silent_push_handler! in your src-tauri crate, link libapp.a into the extension, and add the -Wl,-u,_tauri_notifications_process_silent_push linker flag?",
        log: Self.log, type: .fault)
      deliver(nil)
      return
    }

    let data = Self.flattenCustomKeys(userInfo)
    guard
      let dataJSON = try? JSONSerialization.data(withJSONObject: data),
      let jsonString = String(data: dataJSON, encoding: .utf8)
    else {
      deliver(nil)
      return
    }

    var resultJSON: String?
    dataDirectory().withCString { dirPtr in
      jsonString.withCString { jsonPtr in
        if let raw = process(dirPtr, jsonPtr) {
          resultJSON = String(cString: raw)
          free(raw)
        }
      }
    }

    guard let resultJSON = resultJSON, let resultData = resultJSON.data(using: .utf8) else {
      // The handler declined (returned None) or failed; Rust already logged why.
      deliver(nil)
      return
    }

    do {
      var content = try JSONDecoder().decode(SilentPushContent.self, from: resultData)
      // `extra` values can be any JSON, so they come from JSONSerialization
      // rather than Codable: its NSNumber keeps the exact text of large
      // integers (Matrix timestamps, snowflake ids), where a Double
      // round-trip corrupts anything ≥ 2^53.
      if let object = try JSONSerialization.jsonObject(with: resultData) as? [String: Any],
        let extra = object["extra"] as? [String: Any]
      {
        content.extraStrings = extra.compactMapValues(Self.stringify)
      }
      deliver(content)
    } catch {
      os_log(
        "Failed to decode the handler's notification JSON: %{public}@", log: Self.log,
        type: .error, String(describing: error))
      deliver(nil)
    }
  }

  /// Delivers exactly once, applying `decoded` on top of the original content
  /// when present. `nil` delivers the original (fallback) content unchanged.
  private func deliver(_ decoded: SilentPushContent?) {
    deliveryLock.lock()
    let handler = contentHandler
    contentHandler = nil
    let content = bestAttemptContent ?? UNMutableNotificationContent()
    deliveryLock.unlock()
    guard let handler = handler else { return }

    if let decoded = decoded {
      // `apply` may return a rewritten copy (communication notification).
      handler(decoded.apply(to: content, log: Self.log))
    } else {
      handler(content)
    }
  }

  private func symbol<T>(_ name: String, as type: T.Type) -> T? {
    // The Rust staticlib is linked into the extension executable, so its
    // symbols live in the main image. RTLD_DEFAULT (not importable from Swift,
    // hence the raw -2 handle) searches every loaded image, which also lets
    // tests provide the symbols from their own bundle.
    let rtldDefault = UnsafeMutableRawPointer(bitPattern: -2)
    guard let sym = dlsym(rtldDefault, name) else {
      return nil
    }
    return unsafeBitCast(sym, to: type)
  }

  /// The push payload's top-level custom keys (everything except `aps`),
  /// stringified — the same shape the Android silent-push handler receives.
  static func flattenCustomKeys(_ userInfo: [AnyHashable: Any]) -> [String: String] {
    var data: [String: String] = [:]
    for (key, value) in userInfo {
      guard let key = key as? String, key != "aps" else { continue }
      if let string = stringify(value) {
        data[key] = string
      }
    }
    return data
  }

  /// Canonical value→string rules, shared by the payload's custom keys and the
  /// handler's `extra` values: strings pass through, booleans render as
  /// true/false, numbers via `NSNumber.stringValue` (exact even for integers
  /// beyond 2^53), JSON objects/arrays as their JSON text; JSON null is
  /// skipped (`nil`).
  static func stringify(_ value: Any) -> String? {
    if let string = value as? String {
      return string
    }
    if value is NSNull {
      return nil
    }
    if let number = value as? NSNumber {
      if CFGetTypeID(number) == CFBooleanGetTypeID() {
        return number.boolValue ? "true" : "false"
      }
      return number.stringValue
    }
    if JSONSerialization.isValidJSONObject(value),
      let json = try? JSONSerialization.data(withJSONObject: value),
      let string = String(data: json, encoding: .utf8)
    {
      return string
    }
    return String(describing: value)
  }
}
