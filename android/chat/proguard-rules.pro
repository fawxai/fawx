# Keep Kotlin serialization
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.AnnotationsKt

-keep,includedescriptorclasses class ai.citros.**$$serializer { *; }
-keepclassmembers class ai.citros.** {
    *** Companion;
}
-keepclasseswithmembers class ai.citros.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# OkHttp
-dontwarn okhttp3.**
-dontwarn okio.**
-keep class okhttp3.** { *; }
-keep interface okhttp3.** { *; }

# Keep Compose
-keep class androidx.compose.** { *; }
-dontwarn androidx.compose.**

# Keep core data classes needed for JSON serialization and tool dispatch.
# Only keep @Serializable classes and classes referenced by reflection.
# Using @Keep annotation on individual classes is preferred over blanket keep.
-keep class ai.citros.core.Tool { *; }
-keep class ai.citros.core.ToolCall { *; }
-keep class ai.citros.core.ChatResponse { *; }
-keep class ai.citros.core.ProviderConfig { *; }
-keep class ai.citros.core.WalletState { *; }
-keep class ai.citros.core.WalletKey { *; }
-keep class ai.citros.core.MemoryResult { *; }
-keep class ai.citros.core.MemoryMetadata { *; }
-keep class ai.citros.core.MemoryFilter { *; }
-keep class ai.citros.core.ModelConfig { *; }
-keep class ai.citros.core.ModelCatalog { *; }
-keep class ai.citros.core.ModelCatalog$* { *; }
# Keep enums used in serialization
-keep enum ai.citros.core.Provider { *; }
-keep enum ai.citros.core.ModelTier { *; }
