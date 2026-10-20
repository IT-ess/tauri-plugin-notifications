# Preserve Jackson @JsonValue annotations and the enums that rely on them
# (Importance, Visibility). Without these rules R8 strips the annotation in
# release builds and Jackson falls back to serializing enums as `name()`
# (e.g. "Default") instead of the integer value the Rust side expects.
-keepattributes RuntimeVisibleAnnotations

-keep enum app.tauri.notification.Importance { *; }
-keep enum app.tauri.notification.Visibility { *; }

# The silent-push JNI bridge: the Rust side exports the fixed symbol
# Java_app_tauri_notification_SilentPushNative_process, so R8 must not rename
# or strip the Kotlin class/method it binds to.
-keepclasseswithmembernames class app.tauri.notification.SilentPushNative {
    native <methods>;
}

# Jackson type tokens (`object : TypeReference<…>() {}`) resolve their generic
# supertype reflectively at runtime. R8's vertical class merging removes
# TypeReference when it has a single subclass, which makes that resolution
# throw in release builds (it silently emptied every stored conversation).
-keep class com.fasterxml.jackson.core.type.TypeReference
-keep class * extends com.fasterxml.jackson.core.type.TypeReference
