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

export function skipArgument(bytes: Uint8Array, cursor: number, info: number): number {
  if (info <= 23) return cursor;
  const width = info === 24 ? 1 : info === 25 ? 2 : info === 26 ? 4 : info === 27 ? 8 : 0;
  if (width === 0) throw new MalformedCborSeqError(`reserved additional info ${info}`);
  return requireBytes(bytes, cursor + width);
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

export function requireBytes(bytes: Uint8Array, endOffsetExclusive: number): number {
  if (endOffsetExclusive > bytes.length) {
    throw new NeedMoreBytesError();
  }
  return endOffsetExclusive;
}