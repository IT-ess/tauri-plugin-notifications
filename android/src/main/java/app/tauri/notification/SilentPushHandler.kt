package app.tauri.notification

import android.content.Context

/**
 * Host-provided handler for *silent* (data-only) push messages.
 *
 * Unlike the Rust [`on_silent_push`] channel — which only fires while the Tauri
 * runtime is alive — an implementation of this interface is invoked by
 * [TauriFirebaseMessagingService] **for every data-only message, even when the
 * app has been killed**. In that state Firebase cold-starts the process and runs
 * the messaging service without the Activity/WebView, so there is no Tauri
 * `AppHandle` available: do the fetch here (e.g. via a JNI call into your Rust
 * code) and post the notification with [NotificationPlugin.postBackgroundNotification].
 *
 * Register an implementation by declaring it on the plugin's messaging service in
 * your app's `AndroidManifest.xml`:
 *
 * ```xml
 * <service
 *     android:name="app.tauri.notification.TauriFirebaseMessagingService"
 *     tools:node="merge">
 *     <meta-data
 *         android:name="app.tauri.notification.SILENT_PUSH_HANDLER"
 *         android:value="com.example.MySilentPushHandler" />
 * </service>
 * ```
 *
 * The class must have a public no-argument constructor.
 */
interface SilentPushHandler {
  /**
   * Called on the Firebase background thread for a data-only push.
   *
   * @param context an application [Context]; the Activity is not available here.
   * @param dataDir absolute path of the app's data directory
   *   ([Context.getDataDir]). This is the same location Tauri's path API resolves
   *   to on Android, so a background fetch can open the same on-disk store (e.g. a
   *   Matrix SDK database) the main app uses — useful because the Tauri runtime,
   *   and therefore its path API, is not available after a cold start.
   * @param data the FCM data payload.
   * @param messageId the FCM message id, if present.
   * @return `true` if this handler consumed the message. Returning `true`
   *   suppresses the warm-path Rust `on_silent_push` dispatch so the message is
   *   not handled twice.
   */
  fun onSilentPush(
    context: Context,
    dataDir: String,
    data: Map<String, String>,
    messageId: String?
  ): Boolean
}
