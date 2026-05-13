import ExpoModulesCore

// Symbols exported by `examples/embedded-expo/native` (the cratestack_dispatch
// C ABI). They're linked into the app statically via `libembedded_expo_native.a`,
// declared in `CratestackNotes.podspec`'s `s.vendored_libraries`.
@_silgen_name("cratestack_init")
func cratestack_init(_ dbPath: UnsafePointer<CChar>) -> Int32

@_silgen_name("cratestack_dispatch")
func cratestack_dispatch(
  _ inPtr: UnsafePointer<UInt8>,
  _ inLen: Int,
  _ outPtr: UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>,
  _ outLen: UnsafeMutablePointer<Int>
)

@_silgen_name("cratestack_free")
func cratestack_free(_ ptr: UnsafeMutablePointer<UInt8>?, _ len: Int)

public class CratestackNotesModule: Module {
  public func definition() -> ModuleDefinition {
    Name("CratestackNotes")

    AsyncFunction("initDatabase") { (dbPath: String) -> Void in
      let result = dbPath.withCString { cstr in
        cratestack_init(cstr)
      }
      if result != 0 {
        throw NSError(
          domain: "CratestackNotes",
          code: Int(result),
          userInfo: [NSLocalizedDescriptionKey: "cratestack_init failed (code \(result))"]
        )
      }
    }

    AsyncFunction("dispatch") { (requestJson: String) -> String in
      // Copy the request bytes, hand a pointer to Rust. Rust writes the
      // response into a heap-allocated buffer; we own it after the call
      // returns and must release it via `cratestack_free`.
      let inBytes = Array(requestJson.utf8)
      var outPtr: UnsafeMutablePointer<UInt8>? = nil
      var outLen: Int = 0
      inBytes.withUnsafeBufferPointer { inBuf in
        cratestack_dispatch(inBuf.baseAddress!, inBuf.count, &outPtr, &outLen)
      }
      guard let outBytes = outPtr, outLen > 0 else {
        return "{\"status\":\"err\",\"code\":\"empty_response\",\"message\":\"dispatch returned an empty buffer\"}"
      }
      defer {
        cratestack_free(outBytes, outLen)
      }
      let data = Data(bytes: outBytes, count: outLen)
      return String(data: data, encoding: .utf8)
        ?? "{\"status\":\"err\",\"code\":\"non_utf8_response\",\"message\":\"native returned non-UTF-8 bytes\"}"
    }
  }
}
