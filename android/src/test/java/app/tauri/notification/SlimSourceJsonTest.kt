package app.tauri.notification

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * [slimSourceJson] guards every PendingIntent extra against the ~1MB Binder
 * limit by stripping the base64-heavy conversation fields while keeping the
 * fields the notificationClicked event needs.
 */
@RunWith(RobolectricTestRunner::class)
class SlimSourceJsonTest {

  @Test
  fun stripsHeavyConversationFields() {
    val json = JSONObject()
      .put("id", 7)
      .put("title", "Alice")
      .put("extra", JSONObject().put("deepLink", "matrix:roomid/r/e/x"))
      .put("deepLink", "matrix:roomid/r/e/x")
      .put("conversationAvatarBytes", "A".repeat(200_000))
      .put("messages", listOf(mapOf("text" to "hi", "avatarBytes" to "B".repeat(200_000))))
      .toString()

    val slimmed = JSONObject(slimSourceJson(json)!!)

    assertFalse(slimmed.has("messages"))
    assertFalse(slimmed.has("conversationAvatarBytes"))
    assertEquals(7, slimmed.getInt("id"))
    assertEquals("Alice", slimmed.getString("title"))
    assertEquals("matrix:roomid/r/e/x", slimmed.getString("deepLink"))
    assertEquals("matrix:roomid/r/e/x", slimmed.getJSONObject("extra").getString("deepLink"))
    assertTrue(slimmed.toString().length < 1_000)
  }

  @Test
  fun passesThroughMalformedJsonAndNull() {
    assertEquals("not json", slimSourceJson("not json"))
    assertNull(slimSourceJson(null))
  }
}
