// Pinned local copy of the low-level single-CBOR-item structural walker
// generated into every CrateStack `transport rpc` project by
// `crates/cratestack-client-typescript/templates/src/rpc-cbor-item.ts.j2`
// (issue #277) — kept in sync with that template manually, the same way
// `RpcLink` in `./index.ts` is kept in sync with `rpc-links.ts.j2`. See
// that template file for the full design rationale (ported from
// `crates/cratestack-client-rust/src/streaming.rs`'s use of
// `minicbor::Decoder::skip` deliberately, not reimplemented from a CBOR
// spec reading).
//
// Internal to this package (not part of the `.`/`./cbor-seq` public
// exports) — `./cbor-seq` is the public surface for boundary-scanning;
// this file only exists so `packages/cratestack-ts-types/tests/cbor-seq.test.ts`
// can exercise the real algorithm shape directly, same as `./cbor-seq`
// does for the stateful half.

/** A cbor-seq body the walker can't make sense of at all — a reserved
 *  additional-info value, an indefinite-length string chunk of the
 *  wrong major type, a stray "break" byte with nothing open, etc. This
 *  is always a hard failure: unlike running out of bytes mid-item, more
 *  data arriving later can never fix it. */
export class MalformedCborSeqError extends Error {}

/** Not enough bytes yet to know where the current item ends. Callers
 *  (`CborSeqBoundaryScanner.feedChunk`) catch this and just wait for
 *  the next chunk — it is never a real failure. */
export class NeedMoreBytesError extends Error {}

/** Advance past exactly one top-level CBOR data item starting at
 *  `offset`, returning the offset just past it. Handles every major
 *  type, including indefinite-length byte strings, text strings,
 *  arrays, and maps (RFC 8949 §3.2.1-§3.2.3), recursively — the same
 *  structural cases `minicbor::Decoder::skip` covers on the Rust side.
 *  Never interprets a value — only walks structure. */
export function skipItem(bytes: Uint8Array, offset: number): number {
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
    return skipIndefinite(bytes, cursor, majorType);
  }

  const argument = readArgument(bytes, cursor, additionalInfo);
  switch (majorType) {
    case 0: // unsigned integer — the argument IS the value, no payload
    case 1: // negative integer
      return argument.next;
    case 2: // byte string
    case 3: // text string
      return requireBytes(bytes, argument.next + argument.value);
    case 4: // array — `argument.value` items follow
      return skipCount(bytes, argument.next, argument.value, 1);
    case 5: // map — `argument.value` key/value pairs follow
      return skipCount(bytes, argument.next, argument.value, 2);
    case 6: // tag — exactly one nested item follows the tag number
      return skipItem(bytes, argument.next);
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
): number {
  let cursor = start;
  for (let i = 0; i < count * itemsPerElement; i++) {
    cursor = skipItem(bytes, cursor);
  }
  return cursor;
}

function skipSimpleOrFloat(bytes: Uint8Array, cursor: number, additionalInfo: number): number {
  if (additionalInfo <= 23) {
    return cursor; // simple value encoded inline in the initial byte
  }
  switch (additionalInfo) {
    case 24:
      return requireBytes(bytes, cursor + 1); // 1-byte simple value
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
function skipIndefinite(bytes: Uint8Array, start: number, majorType: number): number {
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
    cursor = skipItem(bytes, cursor); // array element, map key, or string chunk
    if (majorType === 5) {
      cursor = skipItem(bytes, cursor); // map value half of the pair
    }
  }
}

/** Read the argument (length/count/tag-number/value) encoded by
 *  `additionalInfo` starting at `offset`. `additionalInfo <= 23` means
 *  it's inline in the initial byte already consumed by the caller; 24,
 *  25, 26, 27 mean 1, 2, 4, 8 big-endian bytes follow respectively.
 *  Exported so `./cbor-seq`'s tag-header detection can reuse the exact
 *  same argument-decoding logic rather than a second copy of it. */
export function readArgument(
  bytes: Uint8Array,
  offset: number,
  additionalInfo: number,
): { value: number; next: number } {
  if (additionalInfo <= 23) {
    return { value: additionalInfo, next: offset };
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  switch (additionalInfo) {
    case 24:
      requireBytes(bytes, offset + 1);
      return { value: view.getUint8(offset), next: offset + 1 };
    case 25:
      requireBytes(bytes, offset + 2);
      return { value: view.getUint16(offset, false), next: offset + 2 };
    case 26:
      requireBytes(bytes, offset + 4);
      return { value: view.getUint32(offset, false), next: offset + 4 };
    case 27: {
      requireBytes(bytes, offset + 8);
      const big = view.getBigUint64(offset, false);
      if (big > BigInt(Number.MAX_SAFE_INTEGER)) {
        throw new MalformedCborSeqError(
          "cbor-seq item length/count exceeds what this scanner supports",
        );
      }
      return { value: Number(big), next: offset + 8 };
    }
    default:
      // 28-30 are reserved; 31 (indefinite) is handled by the caller
      // before `readArgument` is ever reached.
      throw new MalformedCborSeqError(`reserved additional info ${additionalInfo}`);
  }
}

function requireBytes(bytes: Uint8Array, endOffsetExclusive: number): number {
  if (endOffsetExclusive > bytes.length) {
    throw new NeedMoreBytesError();
  }
  return endOffsetExclusive;
}
