// Thin TypeScript wrapper around the native N-API addon compiled from
// crates/cratestack-cbor-napi (issue #286): assembles its exported
// encode/decode functions into the CratestackRpcCodec shape
// (@cratestack/ts-types) generated CrateStack TypeScript RPC clients
// expect. All CBOR encode/decode logic lives in Rust — see
// crates/cratestack-codec-cbor (the wrapped codec) and
// crates/cratestack-cbor-napi (the FFI boundary); nothing here
// reimplements or alters that behavior.
import type { CratestackRpcCodec } from "@cratestack/ts-types";
import {
  decode as decodeNative,
  encode as encodeNative,
  CompactKeys as NativeCompactKeys,
} from "../native.mjs";

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
    return decodeWith(bytes, undefined);
  },
} satisfies CratestackRpcCodec;

/** Options for {@link createCborCodec}. */
export interface CborCodecOptions {
  /**
   * Maximum nesting of arrays, maps and tags a `decode` accepts (a scalar is
   * depth 0). Defaults to 128. Deeper input throws a `RangeError` whose
   * message contains "nesting too deep"; the codec stays usable.
   */
  maxDepth?: number;
}

// The native addon reports every failure as a plain `Error`. A depth refusal
// is a limit being exceeded, so surface it as the `RangeError` JS callers
// expect (cratestack#1315).
function decodeWith(bytes: Uint8Array, maxDepth: number | undefined): unknown {
  try {
    return decodeNative(bytes, maxDepth);
  } catch (error) {
    if (error instanceof Error && error.message.includes("nesting too deep")) {
      throw new RangeError(error.message, { cause: error });
    }
    throw error;
  }
}

/**
 * Builds a codec with a custom decode depth limit.
 *
 * @example
 * ```ts
 * const codec = createCborCodec({ maxDepth: 32 });
 * ```
 */
export function createCborCodec(options: CborCodecOptions = {}) {
  const { maxDepth } = options;
  if (maxDepth !== undefined && !(Number.isInteger(maxDepth) && maxDepth >= 0)) {
    throw new RangeError("maxDepth must be a non-negative integer");
  }
  return {
    contentType: "application/cbor",
    encode(value: unknown): BodyInit {
      return cborCodec.encode(value);
    },
    decode(bytes: Uint8Array): unknown {
      return decodeWith(bytes, maxDepth);
    },
  } satisfies CratestackRpcCodec;
}

export default cborCodec;

/** Which side of an op a body belongs to. */
export type CompactDirection = "request" | "response";

/**
 * The compact-wire-key tables of one client (`docs/design/compact-wire-keys.md`):
 * swaps schema field names for integer keys on the wire and back. Pure
 * functions of bytes; a bad input throws a catchable `Error`.
 */
export interface CompactKeys {
  /** Named CBOR bytes to compact CBOR bytes. */
  toCompact(named: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
  /** Compact CBOR bytes back to named CBOR bytes. */
  fromCompact(compact: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
  /** Named JSON text to compact JSON text (keys become decimal strings). */
  toCompactJson(named: string, op: string, direction: CompactDirection): string;
  /** Compact JSON text back to named JSON text. */
  fromCompactJson(compact: string, op: string, direction: CompactDirection): string;
  /** The `Cratestack-Contract` header value of the op's current contract. */
  selector(op: string): string | undefined;
}

/**
 * Builds the {@link CompactKeys} of a generated client's key-table artifact
 * (its `KEY_TABLES`). Async for call-site parity with
 * `@cratestack/cbor-web`; the native addon needs no initialization.
 */
export async function createCompactKeys(tables: Uint8Array): Promise<CompactKeys> {
  const keys = NativeCompactKeys.fromTables(tables);
  return {
    // `new Uint8Array(...)` re-wraps for the same typed-array-generics
    // reason as `cborCodec.encode` above.
    toCompact: (named, op, direction) => new Uint8Array(keys.toCompact(named, op, direction)),
    fromCompact: (compact, op, direction) =>
      new Uint8Array(keys.fromCompact(compact, op, direction)),
    toCompactJson: (named, op, direction) => keys.toCompactJson(named, op, direction),
    fromCompactJson: (compact, op, direction) => keys.fromCompactJson(compact, op, direction),
    selector: (op) => keys.selector(op) ?? undefined,
  };
}
