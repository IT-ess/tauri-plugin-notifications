import UserNotifications
import XCTest

@testable import TauriPluginNotificationsNSE

// Rust-side stand-ins: in a real app the host's staticlib exports these C
// symbols via `ios_silent_push_handler!`. Here the test bundle exports the
// same symbols, so `TauriNotificationService`'s dlsym lookup resolves against
// them and the whole didReceive flow can run without a device or APNs.

private var receivedDataDir: String?
private var receivedDataJSON: String?
/// JSON the fake Rust handler returns; `nil` → NULL (handler declined).
private var stubbedResponse: String?

@_cdecl("tauri_notifications_process_silent_push")
func stubProcessSilentPush(
  _ dataDir: UnsafePointer<CChar>?, _ dataJson: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>? {
  receivedDataDir = dataDir.map { String(cString: $0) }
  receivedDataJSON = dataJson.map { String(cString: $0) }
  guard let response = stubbedResponse else { return nil }
  return strdup(response)
}

@_cdecl("tauri_notifications_silent_push_free")
func stubSilentPushFree(_ ptr: UnsafeMutablePointer<CChar>?) {
  free(ptr)
}

final class TauriNotificationServiceTests: XCTestCase {

  override func setUp() {
    super.setUp()
    receivedDataDir = nil
    receivedDataJSON = nil
    stubbedResponse = nil
  }

  /// The Matrix-style push this plugin is built around: a fallback alert plus
  /// `room_id`/`event_id` custom keys.
  private func makeMatrixRequest() -> UNNotificationRequest {
    let content = UNMutableNotificationContent()
    content.title = ""
    content.body = "SINGLE_UNREAD"
    content.userInfo = [
      "aps": [
        "mutable-content": 1,
        "content-available": 1,
        "alert": ["loc-key": "SINGLE_UNREAD", "loc-args": []],
      ],
      "room_id": "!abc:matrix.org",
      "event_id": "$xyz",
    ]
    return UNNotificationRequest(identifier: "test", content: content, trigger: nil)
  }

  private func runService(_ request: UNNotificationRequest) -> UNNotificationContent {
    let service = TauriNotificationService()
    let expectation = expectation(description: "contentHandler called")
    var delivered: UNNotificationContent?
    service.didReceive(request) { content in
      delivered = content
      expectation.fulfill()
    }
    wait(for: [expectation], timeout: 5)
    guard let delivered = delivered else {
      XCTFail("contentHandler was not called")
      return UNNotificationContent()
    }
    return delivered
  }

  func testRewritesContentFromRustHandlerJSON() {
    stubbedResponse = """
      {
        "id": 7,
        "title": "Alice",
        "body": "Hey! Are you around later?",
        "group": "!abc:matrix.org",
        "summary": "!abc:matrix.org",
        "actionTypeId": "message",
        "sound": "ping.caf",
        "extra": {
          "deepLink": "matrix:roomid/abc:matrix.org/e/xyz",
          "unreadCount": 2,
          "encrypted": true
        },
        "messages": [{"sender": "Alice", "text": "ignored on iOS"}],
        "channelId": "android-only-ignored"
      }
      """

    let delivered = runService(makeMatrixRequest())

    // The Rust handler received the flattened custom keys and a data dir.
    XCTAssertNotNil(receivedDataDir)
    XCTAssertFalse(receivedDataDir?.isEmpty ?? true)
    let data = try? JSONSerialization.jsonObject(
      with: Data((receivedDataJSON ?? "").utf8)) as? [String: String]
    XCTAssertEqual(data, ["room_id": "!abc:matrix.org", "event_id": "$xyz"])

    // Content rewritten from the returned NotificationData JSON.
    XCTAssertEqual(delivered.title, "Alice")
    XCTAssertEqual(delivered.body, "Hey! Are you around later?")
    XCTAssertEqual(delivered.threadIdentifier, "!abc:matrix.org")
    XCTAssertEqual(delivered.summaryArgument, "!abc:matrix.org")
    XCTAssertEqual(delivered.categoryIdentifier, "message")
    XCTAssertNotNil(delivered.sound)

    // `extra` merged into userInfo (stringified), original push keys kept —
    // this is what the plugin's notificationClicked handler forwards on tap.
    XCTAssertEqual(delivered.userInfo["deepLink"] as? String, "matrix:roomid/abc:matrix.org/e/xyz")
    XCTAssertEqual(delivered.userInfo["unreadCount"] as? String, "2")
    XCTAssertEqual(delivered.userInfo["encrypted"] as? String, "true")
    XCTAssertEqual(delivered.userInfo["room_id"] as? String, "!abc:matrix.org")
    XCTAssertEqual(delivered.userInfo["event_id"] as? String, "$xyz")
  }

  func testHandlerDeclineDeliversFallbackContent() {
    stubbedResponse = nil  // Rust returns NULL (e.g. no room_id in the payload)

    let delivered = runService(makeMatrixRequest())

    XCTAssertEqual(delivered.body, "SINGLE_UNREAD")
    XCTAssertEqual(delivered.userInfo["room_id"] as? String, "!abc:matrix.org")
  }

  func testMalformedHandlerJSONDeliversFallbackContent() {
    stubbedResponse = "not json at all"

    let delivered = runService(makeMatrixRequest())

    XCTAssertEqual(delivered.body, "SINGLE_UNREAD")
  }

  func testPartialResponseKeepsFallbackFieldsItOmits() {
    stubbedResponse = #"{"title": "Alice"}"#

    let delivered = runService(makeMatrixRequest())

    XCTAssertEqual(delivered.title, "Alice")
    // Body untouched → the payload's own alert text remains.
    XCTAssertEqual(delivered.body, "SINGLE_UNREAD")
  }

  // MARK: - flattenCustomKeys

  func testFlattenCustomKeysMirrorsAndroidDataMap() {
    let flattened = TauriNotificationService.flattenCustomKeys([
      "aps": ["alert": "excluded"],
      "room_id": "!r:hs",
      "count": NSNumber(value: 3),
      "important": NSNumber(value: true),
      "nested": ["a": 1],
    ])

    XCTAssertNil(flattened["aps"])
    XCTAssertEqual(flattened["room_id"], "!r:hs")
    XCTAssertEqual(flattened["count"], "3")
    XCTAssertEqual(flattened["important"], "true")
    XCTAssertEqual(flattened["nested"], #"{"a":1}"#)
  }
}
