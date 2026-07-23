package app.tauri.notification

import android.content.ComponentName
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import app.tauri.plugin.JSObject
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject

/**
 * Manifest meta-data key (on this service) naming the app's native library —
 * the string passed to [System.loadLibrary], i.e. the cargo `[lib]` name
 * without the `lib` prefix or `.so` suffix. When present, data-only pushes are
 * handed to the Rust handler the app registered with the plugin's
 * `silent_push_handler!` macro, in every app state including cold starts.
 */
const val SILENT_PUSH_LIB_META = "app.tauri.notification.SILENT_PUSH_LIB"

class TauriFirebaseMessagingService : FirebaseMessagingService() {

  companion object {
    // Cached System.loadLibrary outcome: null = not attempted yet. Cached so a
    // broken library logs once instead of retrying on every push.
    @Volatile private var libLoaded: Boolean? = null

    @Synchronized
    private fun ensureLibLoaded(name: String): Boolean {
      libLoaded?.let { return it }
      val ok = try {
        System.loadLibrary(name)
        true
      } catch (t: Throwable) {
        Log.e(TAG, "failed to load native library '$name' for silent push handling", t)
        false
      }
      libLoaded = ok
      return ok
    }
  }

  override fun onNewToken(token: String) {
    super.onNewToken(token)
    // Store the token for later retrieval and trigger push-token event
    NotificationPlugin.instance?.handleNewToken(token)
  }

  override fun onMessageReceived(message: RemoteMessage) {
    super.onMessageReceived(message)

    // Plain android.util.Log (not app.tauri.Logger) throughout this service: it
    // must stay visible in a Firebase cold-started process where the Tauri
    // runtime never initialized. `original != priority` exposes an FCM priority
    // downgrade; `warm=false` means the process was started just for this push.
    Log.d(
      TAG,
      "FCM message: id=${message.messageId} priority=${message.priority}" +
        " (original=${message.originalPriority}) dataKeys=${message.data.keys}" +
        " hasNotification=${message.notification != null}" +
        " warm=${NotificationPlugin.instance != null}"
    )

    // Build push message data from RemoteMessage
    val pushData = mutableMapOf<String, Any>()

    // Add notification data if present
    message.notification?.let { notification ->
      notification.title?.let { pushData["title"] = it }
      notification.body?.let { pushData["body"] = it }
      notification.channelId?.let { pushData["channelId"] = it }
      notification.sound?.let { pushData["sound"] = it }
      notification.tag?.let { pushData["tag"] = it }
    }

    // Add data payload
    if (message.data.isNotEmpty()) {
      pushData["data"] = message.data
    }

    // Add message metadata
    message.messageId?.let { pushData["messageId"] = it }
    message.from?.let { pushData["from"] = it }
    pushData["sentTime"] = message.sentTime

    // Silent (data-only) push: no `notification` block, so Android shows
    // nothing on its own. Hand it to the app's Rust handler first — it runs in
    // every app state, including a cold start — and only fall through to the
    // JS `push-message` event when no handler consumed it, so one push is
    // never processed twice.
    val handled =
      message.notification == null && message.data.isNotEmpty() && processNative(message)
    if (!handled) {
      NotificationPlugin.instance?.triggerPushMessage(pushData)
    }

    // Also auto-show notification if notification payload exists
    val notification = message.notification
    if (notification != null) {
      val notificationData = Notification().apply {
        id = System.currentTimeMillis().toInt()
        title = notification.title ?: ""
        body = notification.body
        channelId = notification.channelId
        sound = notification.sound

        // Add data payload if available
        if (message.data.isNotEmpty()) {
          val extraData = JSObject()
          for ((key, value) in message.data) {
            extraData.put(key, value)
          }
          extra = extraData
        }
      }

      // Trigger notification event for push notification received in foreground
      NotificationPlugin.triggerNotification(notificationData, "push")
    }
  }

  /**
   * Run the app's Rust silent-push handler (see [SilentPushNative]) and act on
   * its response: post the returned notification, or clear all active
   * notifications for the `{"clearAll":true}` directive. Mirrors the iOS NSE
   * flow: the plugin owns the bridge and the decode; the app only provides the
   * Rust handler and the [SILENT_PUSH_LIB_META] meta-data. Returns `true` if
   * the message was consumed; any failure degrades to `false` so it still
   * reaches the JS `push-message` event.
   */
  private fun processNative(message: RemoteMessage): Boolean {
    val lib = silentPushLibName()
    if (lib == null) {
      // Normal for apps without a native handler; the JS event still fires.
      Log.d(TAG, "no SILENT_PUSH_LIB meta-data; silent push not handled natively")
      return false
    }
    if (!ensureLibLoaded(lib)) {
      return false
    }
    val json = try {
      // Matches what Tauri's path API resolves to on Android, so the background
      // fetch can open the same store the main app uses.
      SilentPushNative.process(
        applicationContext,
        applicationContext.dataDir.absolutePath,
        JSONObject(message.data as Map<*, *>).toString()
      )
    } catch (t: Throwable) {
      // Throwable: a library that never invoked `silent_push_handler!` raises
      // UnsatisfiedLinkError here, which must not kill the FCM thread.
      Log.e(TAG, "silent push native handler failed", t)
      null
    } ?: return false

    return try {
      // `SilentPushResponse::ClearActive`: nothing to post — the push means
      // "everything has been read", so clear the shade and drop the stored
      // conversation histories (otherwise the next message would resurrect a
      // cleared thread).
      if (JSONObject(json).optBoolean("clearAll", false)) {
        NotificationManagerCompat.from(applicationContext).cancelAll()
        NotificationPlugin.clearAllConversations(applicationContext)
        Log.i(TAG, "silent push: cleared active notifications and conversations")
        return true
      }
      val notification = NotificationPlugin.mapper.readValue(json, Notification::class.java)
      // Round-trips the handler's payload (most importantly `extra`) to the
      // notificationClicked event; buildIntent slims the heavy fields.
      notification.sourceJson = json
      NotificationPlugin.postBackgroundNotification(applicationContext, notification)
      true
    } catch (e: Exception) {
      Log.e(TAG, "failed to parse/post the silent push notification", e)
      false
    }
  }

  private fun silentPushLibName(): String? {
    return try {
      val component = ComponentName(this, TauriFirebaseMessagingService::class.java)
      val info = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        packageManager.getServiceInfo(
          component,
          PackageManager.ComponentInfoFlags.of(PackageManager.GET_META_DATA.toLong())
        )
      } else {
        @Suppress("DEPRECATION")
        packageManager.getServiceInfo(component, PackageManager.GET_META_DATA)
      }
      info.metaData?.getString(SILENT_PUSH_LIB_META)
    } catch (e: Exception) {
      Log.e(TAG, "failed to read silent push lib meta-data", e)
      null
    }
  }
}
