package app.tauri.notification

import android.content.ContentResolver
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Build
import android.util.Base64
import android.service.notification.StatusBarNotification
import androidx.annotation.RequiresApi
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.JSObject
import com.fasterxml.jackson.core.JsonParser
import com.fasterxml.jackson.databind.DeserializationContext
import com.fasterxml.jackson.databind.JsonDeserializer
import com.fasterxml.jackson.databind.JsonNode
import com.fasterxml.jackson.databind.annotation.JsonDeserialize

/**
 * Jackson can't reflect into `JSObject` (zero bean properties), so loading a
 * persisted notification with non-empty `extra` throws
 * `UnrecognizedPropertyException` on the first dynamic key. This deserializer
 * re-parses the JSON subtree through `JSObject`'s own JSON constructor, which
 * accepts arbitrary keys.
 */
class JSObjectDeserializer : JsonDeserializer<JSObject>() {
  override fun deserialize(p: JsonParser, ctxt: DeserializationContext): JSObject {
    val node: JsonNode = p.readValueAsTree()
    return JSObject(node.toString())
  }
}

@InvokeArg
class Notification {
  var id: Int = 0
  var title: String? = null
  var body: String? = null
  var largeBody: String? = null
  var summary: String? = null
  var sound: String? = null
  var icon: String? = null
  var largeIcon: String? = null
  var iconColor: String? = null
  var actionTypeId: String? = null
  var group: String? = null
  var inboxLines: List<String>? = null
  var isGroupSummary = false
  var isOngoing = false
  var isAutoCancel = false
  @JsonDeserialize(using = JSObjectDeserializer::class)
  var extra: JSObject? = null
  var attachments: List<NotificationAttachment>? = null
  var schedule: NotificationSchedule? = null
  var channelId: String? = null
  var sourceJson: String? = null
  var visibility: Int? = null
  var number: Int? = null
  var silent: Boolean? = null

  // Chat-style (Android MessagingStyle). When `messages` is non-empty the
  // notification is rendered as a conversation with per-sender (circular)
  // avatars instead of the BigText/Inbox styles. See [NotificationMessage].
  var messages: List<NotificationMessage>? = null
  var conversationTitle: String? = null
  var groupConversation: Boolean = false
  // Group conversation (room) avatar as base64 image bytes. Only consumed by
  // the iOS NSE today (group-notification icon); declared so Jackson accepts
  // the key when a cross-platform payload carries it.
  var conversationAvatarBytes: String? = null
  var selfName: String? = null
  // When true (default), posting a MessagingStyle notification whose `id` is
  // already showing appends the new messages to that conversation instead of
  // replacing it. Set false to replace (e.g. once the room has been read).
  var appendMessages: Boolean = true
  // When set, tapping the notification fires an ACTION_VIEW intent for this URI
  // (a deep link, e.g. "matrix:roomid/…") pinned to this app's package, instead
  // of launching the default activity. The app's matching <intent-filter>
  // receives it. See `buildIntent` in TauriNotificationManager.
  var deepLink: String? = null

  fun getSound(context: Context, defaultSound: Int): String? {
    var soundPath: String? = null
    var resId: Int = AssetUtils.RESOURCE_ID_ZERO_VALUE
    val name = AssetUtils.getResourceBaseName(sound)
    if (name != null) {
      resId = AssetUtils.getResourceID(context, name, "raw")
    }
    if (resId == AssetUtils.RESOURCE_ID_ZERO_VALUE) {
      resId = defaultSound
    }
    if (resId != AssetUtils.RESOURCE_ID_ZERO_VALUE) {
      soundPath =
        ContentResolver.SCHEME_ANDROID_RESOURCE + "://" + context.packageName + "/" + resId
    }
    return soundPath
  }

  fun getIconColor(globalColor: String): String {
    // use the one defined local before trying for a globally defined color
    return iconColor ?: globalColor
  }

  fun getSmallIcon(context: Context, defaultIcon: Int): Int {
    var resId: Int = AssetUtils.RESOURCE_ID_ZERO_VALUE
    if (icon != null) {
      resId = AssetUtils.getResourceID(context, icon, "drawable")
    }
    if (resId == AssetUtils.RESOURCE_ID_ZERO_VALUE) {
      resId = defaultIcon
    }
    return resId
  }

  fun getLargeIcon(context: Context): Bitmap? {
    if (largeIcon != null) {
      val resId: Int = AssetUtils.getResourceID(context, largeIcon, "drawable")
      return BitmapFactory.decodeResource(context.resources, resId)
    }
    return null
  }

  companion object {
    /**
     * Decode a base64-encoded image (PNG/JPEG bytes) into a [Bitmap], or `null`
     * if the input is null or can't be decoded. Used for dynamic avatars that
     * aren't bundled drawables (e.g. a Matrix sender/room avatar fetched in the
     * background and passed through as base64).
     */
    fun decodeBase64Bitmap(base64: String?): Bitmap? {
      if (base64.isNullOrEmpty()) return null
      return try {
        val bytes = Base64.decode(base64, Base64.DEFAULT)
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
      } catch (e: IllegalArgumentException) {
        null
      }
    }

    fun buildNotificationPendingList(notifications: List<Notification>): List<PendingNotification> {
      val pendingNotifications = mutableListOf<PendingNotification>()
      for (notification in notifications) {
        val pendingNotification = PendingNotification().apply {
          id = notification.id
          title = notification.title
          body = notification.body
          schedule = notification.schedule
          extra = notification.extra
        }
        pendingNotifications.add(pendingNotification)
      }
      return pendingNotifications
    }

    @RequiresApi(Build.VERSION_CODES.M)
    fun buildNotificationActiveList(statusBarNotifications: Array<StatusBarNotification>): List<ActiveNotificationInfo> {
      val activeNotifications = mutableListOf<ActiveNotificationInfo>()
      for (statusBarNotification in statusBarNotifications) {
        val notification = statusBarNotification.notification
        val extractedData = mutableMapOf<String, String>()
        if (notification != null) {
          for (key in notification.extras.keySet()) {
            notification.extras.getString(key)?.let { value ->
              extractedData[key] = value
            }
          }
        }

        val activeNotification = ActiveNotificationInfo().apply {
          id = statusBarNotification.id
          tag = statusBarNotification.tag
          title = notification?.extras?.getCharSequence(android.app.Notification.EXTRA_TITLE)?.toString()
          body = notification?.extras?.getCharSequence(android.app.Notification.EXTRA_TEXT)?.toString()
          group = notification?.group
          groupSummary = notification?.let { 0 != it.flags and android.app.Notification.FLAG_GROUP_SUMMARY } ?: false
          data = extractedData
        }
        activeNotifications.add(activeNotification)
      }
      return activeNotifications
    }
  }
}

/**
 * A single chat message rendered inside an Android `MessagingStyle` notification.
 * The [avatarBytes] is the sender's avatar as base64 image bytes (decoded by the
 * plugin), which `MessagingStyle` displays as a circular icon.
 */
@InvokeArg
class NotificationMessage {
  var sender: String? = null
  var personKey: String? = null
  var avatarBytes: String? = null
  var text: String? = null
  var timestamp: Long = 0
}

@InvokeArg
class PendingNotification {
  var id: Int = 0
  var title: String? = null
  var body: String? = null
  var schedule: NotificationSchedule? = null
  @JsonDeserialize(using = JSObjectDeserializer::class)
  var extra: JSObject? = null
}

@InvokeArg
class ActiveNotificationInfo {
  var id: Int = 0
  var tag: String? = null
  var title: String? = null
  var body: String? = null
  var group: String? = null
  var groupSummary: Boolean = false
  var data: Map<String, String> = emptyMap()
  var extra: Map<String, Any> = emptyMap()
  var attachments: List<AttachmentInfo> = emptyList()
  var actionTypeId: String? = null
  var schedule: NotificationSchedule? = null
  var sound: String? = null
}

@InvokeArg
class AttachmentInfo {
  var id: String = ""
  var url: String = ""
}