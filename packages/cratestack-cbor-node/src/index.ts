// Thin TypeScript wrapper around the native N-API addon compiled from
// crates/cratestack-cbor-napi (issue #286): assembles its exported
// encode/decode functions into the CratestackRpcCodec shape
// (@cratestack/ts-types) generated CrateStack TypeScript RPC clients
// expect. All CBOR encode/decode logic lives in Rust — see
// crates/cratestack-codec-cbor (the wrapped codec) and
// crates/cratestack-cbor-napi (the FFI boundary); nothing here
// reimplements or alters that behavior.
import type { CratestackRpcCodec } from "@cratestack/ts-types";
import { decode as decodeNative, encode as encodeNative } from "../native.mjs";

/**
 * CBOR codec backed by `cratestack-codec-cbor`'s Rust `CborCodec` (via the
 * native N-API binding built from `crates/cratestack-cbor-napi`) —
 * byte-identical wire behavior to the framework's own server and Rust
 * client, since it's the same Rust implementation, not a JS
 * reimplementation.
 *
 * `satisfies` (rather than an explicit `: CratestackRpcCodec` annotation)
 * deliberately keeps the *inferred* literal object type in this package's
 * emitted `.d.ts` — consumers get full structural checking against
 * `CratestackRpcCodec` without needing `@cratestack/ts-types` installed
 * themselves just to resolve this export's type.
 */
export const cborCodec = {
  contentType: "application/cbor",
  encode(value: unknown): BodyInit {
    // `new Uint8Array(...)` re-wraps the native return value rather than
    // returning it directly: TS 5.7+'s generic typed arrays default to
    // `Uint8Array<ArrayBufferLike>` (napi's declared return type, with no
    // type argument), but `BodyInit`'s `ArrayBufferView` branch requires
    // the narrower `Uint8Array<ArrayBuffer>` — the constructor overload
    // used here is the one TS pins to that narrower type. Purely a
    // type-level fix: the native array is already a real, JS-owned
    // `ArrayBuffer`-backed `Uint8Array` at runtime (never a
    // `SharedArrayBuffer`), so this only reshapes the static type, not
    // the runtime value's compatibility with `fetch`.
    return new Uint8Array(encodeNative(value));
  },
  decode(bytes: Uint8Array): unknown {
    return decodeNative(bytes);
  },
} satisfies CratestackRpcCodec;

export default cborCodec;
