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
  // symbol is registered by `RegisterNatives` below — we don't use the
  // auto-mangled "Java_..." names because the Rust side is plain
  // `extern "C"` rather than #[no_mangle] JNI-named.
  private external fun nativeInit(dbPath: String): Int
  private external fun nativeDispatch(request: ByteArray): ByteArray

  override fun definition() = ModuleDefinition {
    Name("CratestackNotes")

    AsyncFunction("initDatabase") { dbPath: String ->
      val code = nativeInit(dbPath)
      if (code != 0) {
        throw RuntimeException("cratestack_init failed (code $code)")
      }
    }

    AsyncFunction("dispatch") { requestJson: String ->
      val response = nativeDispatch(requestJson.toByteArray(Charsets.UTF_8))
      String(response, Charsets.UTF_8)
    }
  }
}
