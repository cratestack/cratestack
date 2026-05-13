package dev.cratestack.examples.cratestacknotes

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class CratestackNotesModule : Module() {
  // Loads the native library `libembedded_expo_native.so` (built by
  // cargo-ndk, then dropped into `android/src/main/jniLibs/<abi>/` by
  // the Gradle pre-build hook). The JNI bridge below resolves to the
  // C ABI exported from `examples/embedded-expo/native/src/lib.rs`.
  init {
    System.loadLibrary("embedded_expo_native")
  }

  // `external` declares a JNI-bound function. The matching native
  // symbols are exported from `native/src/lib.rs`'s `android_jni`
  // module using the canonical `Java_<dotted_class>_<methodName>` name
  // mangling, so no `RegisterNatives` call is needed.
  //
  // On failure, the Rust side throws a `java.lang.RuntimeException`
  // with the underlying error message before returning -1, so any
  // caller-side `try { ... }` block sees the real cause instead of a
  // bare "code -1" placeholder.
  private external fun nativeInit(dbPath: String): Int
  private external fun nativeDispatch(request: ByteArray): ByteArray

  override fun definition() = ModuleDefinition {
    Name("CratestackNotes")

    AsyncFunction("initDatabase") { dbPath: String ->
      // The Rust side throws a RuntimeException on failure; we just
      // call through and let it propagate. The Int return is only
      // checked as a belt-and-suspenders guard for the
      // unreachable-in-practice case where Rust returned -1 without
      // raising a JVM exception.
      val code = nativeInit(dbPath)
      if (code != 0) {
        throw RuntimeException("cratestack_init returned $code without throwing")
      }
    }

    AsyncFunction("dispatch") { requestJson: String ->
      val response = nativeDispatch(requestJson.toByteArray(Charsets.UTF_8))
      String(response, Charsets.UTF_8)
    }
  }
}
