// Test-only CBOR value decoder — NOT shipped, NOT the boundary scanner
// under test. `CborSeqBoundaryScanner`/`classifyCborSeqItem` only walk
// CBOR *structure* to find item boundaries and strip a tag header; they
// deliberately delegate actual value decoding to a bring-your-own
// `CratestackRpcCodec.decode()` (the same "bring your own codec" design
// `runtime.ts.j2` already has for `call()`/`batch()`). Real projects
// plug in a real CBOR library there; these tests just need *a* decoder
// that can read the specific small maps of uints/strings the fixtures
// in `tests/fixtures/*.hex` actually contain (`Tick { index, value }`
// and `RpcErrorBody { code, message }`), so a ~40-line general-enough
// decoder here avoids taking on a real CBOR npm dependency (or the
// toolchain-gated `@cratestack/cbor-node`/`-web`) just for test fixtures.
import type { CratestackRpcCodec } from "../../src/index.js";

export const miniCborCodec: CratestackRpcCodec = {
  contentType: "application/cbor",
  encode(): BodyInit {
    // The real request body content never matters to these tests (the
    // mocked `fetchFn` ignores it and returns a canned `Response`), but
    // the terminal stream link always calls `codec.encode()` while
    // building the fetch call, so this needs to return *something*
    // rather than throw.
    return new Uint8Array(0);
  },
  decode(bytes: Uint8Array): unknown {
    const [value] = decodeValue(bytes, 0);
    return value;
  },
};

function decodeValue(bytes: Uint8Array, offset: number): [unknown, number] {
  const initial = bytes[offset]!;
  const majorType = initial >> 5;
  const additionalInfo = initial & 0x1f;
  const [argument, next] = readArg(bytes, offset + 1, additionalInfo);

  switch (majorType) {
    case 0:
      return [argument, next];
    case 1:
      return [-1 - argument, next];
    case 3: {
      const text = new TextDecoder().decode(bytes.slice(next, next + argument));
      return [text, next + argument];
    }
    case 4: {
      const out: unknown[] = [];
      let cursor = next;
      for (let i = 0; i < argument; i++) {
        const [item, after] = decodeValue(bytes, cursor);
        out.push(item);
        cursor = after;
      }
      return [out, cursor];
    }
    case 5: {
      const out: Record<string, unknown> = {};
      let cursor = next;
      for (let i = 0; i < argument; i++) {
        const [key, afterKey] = decodeValue(bytes, cursor);
        const [value, afterValue] = decodeValue(bytes, afterKey);
        out[String(key)] = value;
        cursor = afterValue;
      }
      return [out, cursor];
    }
    case 6:
      // A tag other than RPC_STREAM_ERROR_TAG (already stripped by
      // `classifyCborSeqItem` before `codec.decode` is ever called for
      // that one) — decode straight through to the tagged value,
      // discarding the tag number itself. Good enough for these tests,
      // which only care that decoding SUCCEEDS for a non-sentinel tag.
      return decodeValue(bytes, next);
    default:
      throw new Error(`miniCborCodec: unsupported CBOR major type ${majorType} for test fixtures`);
  }
}

function readArg(bytes: Uint8Array, offset: number, additionalInfo: number): [number, number] {
  if (additionalInfo <= 23) {
    return [additionalInfo, offset];
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  switch (additionalInfo) {
    case 24:
      return [view.getUint8(offset), offset + 1];
    case 25:
      return [view.getUint16(offset, false), offset + 2];
    case 26:
      return [view.getUint32(offset, false), offset + 4];
    default:
      throw new Error(
        `miniCborCodec: unsupported additional info ${additionalInfo} for test fixtures`,
      );
  }
}
