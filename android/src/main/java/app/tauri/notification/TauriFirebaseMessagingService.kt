package app.tauri.notification

import android.content.ComponentName
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import app.tauri.plugin.JSObject
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

const val SILENT_PUSH_HANDLER_META = "app.tauri.notification.SILENT_PUSH_HANDLER"

class TauriFirebaseMessagingService : FirebaseMessagingService() {

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
    Log.i(
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

    // Trigger push-message event
    NotificationPlugin.instance?.triggerPushMessage(pushData)

    // Silent (data-only) push: no `notification` block, so Android shows
    // nothing.
    if (message.notification == null && message.data.isNotEmpty()) {
      // Preferred path: a host-provided background handler declared via
      // <meta-data>. This runs even when the app was killed (cold start), so it
      // can fetch the real content (e.g. a Matrix event by id) and post the
      // notification itself. If it consumes the message we stop here.
      val handled = dispatchToBackgroundHandler(message)

      // Fallback warm path: the Rust `on_silent_push` channel, only live while
      // the Tauri runtime is up. Skipped when the background handler took it, to
      // avoid handling the same message twice.
      if (!handled) {
        NotificationPlugin.instance?.dispatchSilentPush(pushData)
      }
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
   * Resolve the host [SilentPushHandler] declared on this service via
   * `<meta-data android:name="app.tauri.notification.SILENT_PUSH_HANDLER">` and
   * invoke it. Runs regardless of whether the Tauri runtime is up, so it works
   * after a cold start. Returns `true` if a handler consumed the message.
   */
  private fun dispatchToBackgroundHandler(message: RemoteMessage): Boolean {
    val className = silentPushHandlerClassName()
    if (className == null) {
      // A cold-started process has no other way to show the push, so a
      // missing/unreadable meta-data entry must not fail silently.
      Log.w(TAG, "no SILENT_PUSH_HANDLER meta-data resolved; silent push not handled in background")
      return false
    }
    return try {
      val handler = Class.forName(className)
        .getDeclaredConstructor()
        .newInstance() as? SilentPushHandler
      if (handler == null) {
        Log.e(TAG, "$className does not implement SilentPushHandler")
        return false
      }
      // Matches what Tauri's path API resolves to on Android (activity.dataDir),
      // so the background fetch can open the same store the main app uses.
      val dataDir = applicationContext.dataDir.absolutePath
      val handled = handler.onSilentPush(applicationContext, dataDir, message.data, message.messageId)
      Log.i(TAG, "silent push handler $className returned $handled")
      handled
    } catch (e: Throwable) {
      // Throwable, not Exception: Class.forName/class-init can throw
      // LinkageError, which would otherwise escape and kill the FCM thread.
      Log.e(TAG, "silent push handler '$className' failed", e)
      false
    }
  }

  private fun silentPushHandlerClassName(): String? {
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
      info.metaData?.getString(SILENT_PUSH_HANDLER_META)
    } catch (e: Exception) {
      Log.e(TAG, "failed to read silent push handler meta-data", e)
      null
    }
  }
}
