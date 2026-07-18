package app.tauri.notification

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire-contract gate for the silent-push path: [TauriFirebaseMessagingService]
 * parses the Rust handler's `NotificationData` JSON with the shared
 * [NotificationPlugin.mapper]. The fixture below is the literal `serde_json`
 * output of a fully populated `NotificationData` (see the camelCase serde
 * tests in src/models.rs) — if a Rust-side field stops mapping onto
 * [Notification], this test fails before a device ever does.
 */
class NotificationJacksonTest {

  // Literal serde_json output of NotificationData::builder() with every field set.
  private val fixture = """
    {"id":42,"channelId":"default","title":"Alice","body":"hello","schedule":null,
     "largeBody":"large","summary":"!room:matrix.org","actionTypeId":"message",
     "group":"!room:matrix.org","groupSummary":true,"sound":"ping.caf",
     "inboxLines":["line1"],"icon":"ic","largeIcon":"ic_large","iconColor":"#ff0000",
     "attachments":[{"id":"a1","url":"file:///attachment.png"}],
     "extra":{"deepLink":"matrix:roomid/room:matrix.org/e/xyz","count":2},
     "ongoing":true,"autoCancel":true,"silent":true,
     "messages":[{"sender":"Alice","personKey":"@alice:matrix.org","avatarBytes":"aGk=",
                  "text":"Hey!","timestamp":1721270000000}],
     "conversationTitle":"Rust enjoyers","groupConversation":true,
     "conversationAvatarBytes":"cm9vbQ==","selfName":"Me","appendMessages":true,
     "deepLink":"matrix:roomid/room:matrix.org/e/xyz"}
  """.trimIndent()

  @Test
  fun parsesFullSerdeNotificationData() {
    val notification = NotificationPlugin.mapper.readValue(fixture, Notification::class.java)

    assertEquals(42, notification.id)
    assertEquals("default", notification.channelId)
    assertEquals("Alice", notification.title)
    assertEquals("hello", notification.body)
    assertNull(notification.schedule)
    assertEquals("large", notification.largeBody)
    assertEquals("!room:matrix.org", notification.summary)
    assertEquals("message", notification.actionTypeId)
    assertEquals("!room:matrix.org", notification.group)
    assertTrue(notification.isGroupSummary)
    assertEquals("ping.caf", notification.sound)
    assertEquals(listOf("line1"), notification.inboxLines)
    assertEquals("ic", notification.icon)
    assertEquals("ic_large", notification.largeIcon)
    assertEquals("#ff0000", notification.iconColor)
    assertEquals("a1", notification.attachments?.single()?.id)
    assertTrue(notification.isOngoing)
    assertTrue(notification.isAutoCancel)
    assertEquals(true, notification.silent)

    // The MessagingStyle fields the silent-push path exists for.
    val message = notification.messages?.single()
    assertEquals("Alice", message?.sender)
    assertEquals("@alice:matrix.org", message?.personKey)
    assertEquals("aGk=", message?.avatarBytes)
    assertEquals("Hey!", message?.text)
    assertEquals(1721270000000L, message?.timestamp)
    assertEquals("Rust enjoyers", notification.conversationTitle)
    assertTrue(notification.groupConversation)
    assertEquals("cm9vbQ==", notification.conversationAvatarBytes)
    assertEquals("Me", notification.selfName)
    assertTrue(notification.appendMessages)
    assertEquals("matrix:roomid/room:matrix.org/e/xyz", notification.deepLink)
    assertEquals("matrix:roomid/room:matrix.org/e/xyz", notification.extra?.getString("deepLink"))
  }

  @Test
  fun toleratesUnknownFieldsFromNewerRustSide() {
    val json = """{"id":1,"title":"t","futureField":"whatever"}"""

    val notification = NotificationPlugin.mapper.readValue(json, Notification::class.java)

    assertEquals(1, notification.id)
    assertEquals("t", notification.title)
  }
}
