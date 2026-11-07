// Single-CBOR-item structural walker — the low-level half of the
// `application/cbor-seq` boundary scanner (issue #277, see
// `./cbor-seq` for the stateful, chunk-buffering half that consumes
// this). Walks structure, validates text UTF-8 and simple-value encoding,
// and bounds nesting to 256. It does not interpret tag semantics or enforce
// unique map keys; the server codec accepts duplicate keys.
//
// Internal to the generated package (not re-exported from `./index`) —
// `./cbor-seq` is the public surface for boundary-scanning.

import {
  MalformedCborSeqError,
  NeedMoreBytesError,
  readArgument,
  requireBytes,
  skipArgument,
} from "./cbor-argument.js";

export { MalformedCborSeqError, NeedMoreBytesError, readArgument } from "./cbor-argument.js";

/** Advance past exactly one top-level CBOR data item starting at
 *  `offset`, returning the offset just past it. Handles every major
 *  type, including indefinite-length byte strings, text strings,
 *  arrays, and maps (RFC 8949 §3.2.1-§3.2.3), recursively — the same
 *  structural cases `minicbor::Decoder::skip` covers on the Rust side.
 *  It validates text and framing, but not tag semantics or map-key uniqueness. */
export function skipItem(bytes: Uint8Array, offset: number, depth = 0): number {
  if (depth > 256) throw new MalformedCborSeqError("CBOR nesting limit exceeded");
  if (offset >= bytes.length) {
    throw new NeedMoreBytesError();
  }
  const initial = bytes[offset]!;
  const majorType = initial >> 5;
  const additionalInfo = initial & 0x1f;
  const cursor = offset + 1;

  if (majorType === 7) {
    return skipSimpleOrFloat(bytes, cursor, additionalInfo);
  }
  if (additionalInfo === 31) {
    // Indefinite length is only valid for byte/text strings, arrays,
    // and maps (RFC 8949 §3.2.1) — never for an integer or a tag.
    if (majorType === 0 || majorType === 1 || majorType === 6) {
      throw new MalformedCborSeqError(
        `major type ${majorType} cannot use indefinite-length encoding`,
      );
    }
    return skipIndefinite(bytes, cursor, majorType, depth);
  }

  // Scalar values and tags can span all 64 bits: only lengths need a JS number.
  if (majorType === 0 || majorType === 1 || majorType === 6) {
    const next = skipArgument(bytes, cursor, additionalInfo);
    return majorType === 6 ? skipItem(bytes, next, depth + 1) : next;
  }
  const argument = readArgument(bytes, cursor, additionalInfo);
  switch (majorType) {
    case 2: // byte string
      return requireBytes(bytes, argument.next + argument.value);
    case 3: {
      // each definite text chunk must be valid UTF-8
      const end = requireBytes(bytes, argument.next + argument.value);
      try {
        new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(argument.next, end));
      } catch {
        throw new MalformedCborSeqError("invalid CBOR text UTF-8");
      }
      return end;
    }
    case 4: // array — `argument.value` items follow
      return skipCount(bytes, argument.next, argument.value, 1, depth);
    case 5: // map — `argument.value` key/value pairs follow
      return skipCount(bytes, argument.next, argument.value, 2, depth);
    default:
      // Unreachable: majorType is `initial >> 5`, always 0-7.
      throw new MalformedCborSeqError(`unreachable CBOR major type ${majorType}`);
  }
}

function skipCount(
  bytes: Uint8Array,
  start: number,
  count: number,
  itemsPerElement: number,
  depth: number,
): number {
  let cursor = start;
  for (let i = 0; i < count * itemsPerElement; i++) {
    cursor = skipItem(bytes, cursor, depth + 1);
  }
  return cursor;
}

function skipSimpleOrFloat(bytes: Uint8Array, cursor: number, additionalInfo: number): number {
  if (additionalInfo <= 23) {
    return cursor; // simple value encoded inline in the initial byte
  }
  switch (additionalInfo) {
    case 24:
      requireBytes(bytes, cursor + 1);
      if (bytes[cursor]! < 32) throw new MalformedCborSeqError("invalid CBOR simple value");
      return cursor + 1;
    case 25:
      return requireBytes(bytes, cursor + 2); // half-precision float
    case 26:
      return requireBytes(bytes, cursor + 4); // single-precision float
    case 27:
      return requireBytes(bytes, cursor + 8); // double-precision float
    case 31:
      throw new MalformedCborSeqError(
        "unexpected CBOR break code outside an open indefinite-length item",
      );
    default:
      throw new MalformedCborSeqError(`reserved additional info ${additionalInfo} on major type 7`);
  }
}

/** Indefinite-length byte string / text string / array / map (major
 *  types 2, 3, 4, 5 with additional info 31): items until a "break"
 *  byte (`0xff`). Byte/text strings additionally require every chunk to
 *  be a definite-length chunk of the *same* major type (RFC 8949
 *  §3.2.3) — nesting or mixing major types there is malformed. */
function skipIndefinite(
  bytes: Uint8Array,
  start: number,
  majorType: number,
  depth: number,
): number {
  let cursor = start;
  for (;;) {
    if (cursor >= bytes.length) {
      throw new NeedMoreBytesError();
    }
    if (bytes[cursor] === 0xff) {
      return cursor + 1; // break
    }
    if (majorType === 2 || majorType === 3) {
      const chunkMajorType = bytes[cursor]! >> 5;
      const chunkAdditionalInfo = bytes[cursor]! & 0x1f;
      if (chunkMajorType !== majorType) {
        throw new MalformedCborSeqError(
          `indefinite-length string chunk has major type ${chunkMajorType}, expected ${majorType}`,
        );
      }
      if (chunkAdditionalInfo === 31) {
        throw new MalformedCborSeqError(
          "indefinite-length string chunks cannot themselves be indefinite-length",
        );
      }
    }
    cursor = skipItem(bytes, cursor, majorType === 2 || majorType === 3 ? depth : depth + 1);
    if (majorType === 5) {
      cursor = skipItem(bytes, cursor, depth + 1); // map value half of the pair
    }
  }
}
