package app.tauri.notification

import android.content.Context
import android.content.SharedPreferences
import app.tauri.Logger
import com.fasterxml.jackson.databind.ObjectMapper

private const val STORAGE_TAG = "NotificationStorage"
// Key for private preferences
private const val NOTIFICATION_STORE_ID = "NOTIFICATION_STORE"
// Key used to save action types
private const val ACTION_TYPES_ID = "ACTION_TYPE_STORE"
// Key for accumulating MessagingStyle conversations (per notification id) and
// per-sender avatars, so chat notifications survive process death reliably.
private const val MESSAGING_STORE_ID = "MESSAGING_STORE"

class NotificationStorage(private val context: Context, private val jsonMapper: ObjectMapper) {
  fun appendNotifications(localNotifications: List<Notification>) {
    Logger.debug(Logger.tags(STORAGE_TAG), "Appending ${localNotifications.size} notifications to storage")
    val storage = getStorage(NOTIFICATION_STORE_ID)
    val editor = storage.edit()
    var savedCount = 0
    for (request in localNotifications) {
      if (request.schedule != null) {
        val key: String = request.id.toString()
        val jsonValue = request.sourceJson
        Logger.debug(Logger.tags(STORAGE_TAG), "Saving notification $key, sourceJson is null: ${request.sourceJson == null}, value: ${jsonValue?.take(100)}")
        editor.putString(key, jsonValue)
        savedCount++
      } else {
        Logger.debug(Logger.tags(STORAGE_TAG), "Skipping notification ${request.id} - no schedule")
      }
    }
    editor.apply()
    Logger.debug(Logger.tags(STORAGE_TAG), "Actually saved $savedCount scheduled notifications")
  }

  fun getSavedNotificationIds(): List<String> {
    val storage = getStorage(NOTIFICATION_STORE_ID)
    val all = storage.all
    val ids = if (all != null) ArrayList(all.keys) else ArrayList()
    Logger.debug(Logger.tags(STORAGE_TAG), "Retrieved ${ids.size} saved notification IDs")
    return ids
  }

  fun getSavedNotifications(): List<Notification> {
    val storage = getStorage(NOTIFICATION_STORE_ID)
    val all = storage.all
    Logger.debug(Logger.tags(STORAGE_TAG), "Storage keys: ${all?.keys}")
    val notifications = all?.keys?.mapNotNull { key ->
      val value = all[key]
      Logger.debug(Logger.tags(STORAGE_TAG), "Key $key, value type: ${value?.javaClass?.name}, value: ${value.toString().take(100)}")
      val json = value as? String
      parseNotification(json)
    } ?: emptyList()
    Logger.debug(Logger.tags(STORAGE_TAG), "Retrieved ${notifications.size} saved notifications")
    return notifications
  }

  fun getSavedNotification(key: String): Notification? {
    val storage = getStorage(NOTIFICATION_STORE_ID)
    val notificationString = try {
      storage.getString(key, null)
    } catch (e: ClassCastException) {
      Logger.error(Logger.tags(STORAGE_TAG), "Failed to get notification string for key $key: ${e.message}", e)
      return null
    }
    return parseNotification(notificationString)
  }

  private fun parseNotification(json: String?): Notification? {
    if (json == null) return null
    return try {
      jsonMapper.readValue(json, Notification::class.java)
    } catch (e: Exception) {
      Logger.error(Logger.tags(STORAGE_TAG), "Failed to parse notification: ${e.message}", e)
      null
    }
  }

  fun deleteNotification(id: String?) {
    Logger.debug(Logger.tags(STORAGE_TAG), "Deleting notification with id: $id")
    val editor = getStorage(NOTIFICATION_STORE_ID).edit()
    editor.remove(id)
    editor.apply()
  }

  private fun getStorage(key: String): SharedPreferences {
    return context.getSharedPreferences(key, Context.MODE_PRIVATE)
  }

  /** Load the accumulated chat messages for a conversation (notification id). */
  fun loadConversation(id: Int): MutableList<NotificationMessage> {
    val json = getStorage(MESSAGING_STORE_ID).getString("conv_$id", null) ?: return mutableListOf()
    // The list type is built programmatically instead of with an anonymous
    // `TypeReference<…>() {}`: R8 vertically merges TypeReference into its sole
    // subclass in release builds, which breaks the getGenericSuperclass() type
    // discovery and made this load throw (and return empty) on every call.
    val listType =
      jsonMapper.typeFactory.constructCollectionType(ArrayList::class.java, NotificationMessage::class.java)
    return try {
      jsonMapper.readValue(json, listType)
    } catch (e: Exception) {
      Logger.error(Logger.tags(STORAGE_TAG), "Failed to parse conversation $id: ${e.message}", e)
      mutableListOf()
    }
  }

  /** Persist the (capped) chat messages for a conversation. */
  fun saveConversation(id: Int, messages: List<NotificationMessage>) {
    val json = try {
      jsonMapper.writeValueAsString(messages)
    } catch (e: Exception) {
      Logger.error(Logger.tags(STORAGE_TAG), "Failed to serialize conversation $id: ${e.message}", e)
      return
    }
    getStorage(MESSAGING_STORE_ID).edit().putString("conv_$id", json).apply()
  }

  /** Drop a conversation's history (e.g. when its notification is cancelled). */
  fun clearConversation(id: Int) {
    getStorage(MESSAGING_STORE_ID).edit().remove("conv_$id").remove("conv_avatar_$id").apply()
  }

  /** Drop every conversation's history and all stored avatars. */
  fun clearAllConversations() {
    // The messaging store only holds conv_*/conv_avatar_*/avatar_* keys, so a
    // full clear is both simpler and the only path that frees sender avatars.
    getStorage(MESSAGING_STORE_ID).edit().clear().apply()
  }

  /** Store the group conversation's (room's) avatar for a notification id. */
  fun saveConversationAvatar(id: Int, base64: String) {
    getStorage(MESSAGING_STORE_ID).edit().putString("conv_avatar_$id", base64).apply()
  }

  fun loadConversationAvatar(id: Int): String? =
    getStorage(MESSAGING_STORE_ID).getString("conv_avatar_$id", null)

  /** Store a sender's avatar once (by person key), referenced by their messages. */
  fun saveAvatar(personKey: String, base64: String) {
    getStorage(MESSAGING_STORE_ID).edit().putString("avatar_$personKey", base64).apply()
  }

  fun loadAvatar(personKey: String?): String? {
    if (personKey == null) return null
    return getStorage(MESSAGING_STORE_ID).getString("avatar_$personKey", null)
  }

  fun writeActionGroup(actions: List<ActionType>) {
    for (type in actions) {
      val editor = getStorage(ACTION_TYPES_ID + type.id).edit()
      editor.clear()
      editor.putInt("count", type.actions.size)
      for ((index, action) in type.actions.withIndex()) {
        editor.putString("id$index", action.id)
        editor.putString("title$index", action.title)
        editor.putBoolean("input$index", action.input ?: false)
      }
      editor.apply()
      Logger.debug(Logger.tags(STORAGE_TAG), "Saved action group ${type.id} with ${type.actions.size} actions")
    }
  }

  fun getActionGroup(forId: String): Array<NotificationAction?> {
    val storage = getStorage(ACTION_TYPES_ID + forId)
    val count = storage.getInt("count", 0)
    Logger.debug(Logger.tags(STORAGE_TAG), "Getting action group $forId, count: $count")
    val actions: Array<NotificationAction?> = arrayOfNulls(count)
    for (i in 0 until count) {
      val id = storage.getString("id$i", "")
      val title = storage.getString("title$i", "")
      val input = storage.getBoolean("input$i", false)
      Logger.debug(Logger.tags(STORAGE_TAG), "Action $i: id=$id, title=$title, input=$input")

      val action = NotificationAction()
      action.id = id ?: ""
      action.title = title
      action.input = input
      actions[i] = action
    }
    return actions
  }
}