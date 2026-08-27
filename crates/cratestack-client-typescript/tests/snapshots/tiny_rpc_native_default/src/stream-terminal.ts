// The `streamLinks` chain's terminal link (issue #277) — performs the
// real network call for `CratestackRpcRuntime.stream()` and turns the
// response into an `AsyncIterable<RpcStreamFrame>`. Always runs last
// regardless of what `streamLinks` declares; split out of `./runtime`
// to keep that file from growing past its already-over-budget size —
// see that file's own `terminalLink` for the `call()`/`batch()`
// equivalent this mirrors.
//
// A non-2xx response throws `CratestackRpcError` directly (same as
// `call()`/`batch()`, and same as `stream()` did before issue #277)
// rather than yielding an error frame — `RpcStreamFrame`'s `"error"`
// kind is specifically the *mid-stream* sentinel (issue #281), which
// can only ever happen after a `200` was already sent; an outright
// failed request is an ordinary HTTP-level error.

import {
  CBOR_SEQ_CONTENT_TYPE,
  CratestackRpcError,
  CratestackRpcTransportError,
  matchesContentType,
  readErrorBody,
} from "./runtime.js";
import type { RpcStreamLinkNext } from "./links.js";
import { CborSeqBoundaryScanner, classifyCborSeqItem } from "./cbor-seq.js";
import { encodeDecimalFields } from "./models.js";

// `encodeDecimalFields(request.input)` runs immediately before
// `codec.encode()` — the `stream()` counterpart of `./runtime.js`'s
// `terminalLink` doing the same for `call()`/`batch()`. See
// `encodeDecimalFields`'s own doc comment (`models.ts.j2`) for why.
export const terminalStreamLink: RpcStreamLinkNext = async function* (request) {
  const response = await request.fetchFn(request.url, {
    method: "POST",
    headers: request.headers,
    body: request.codec.encode(encodeDecimalFields(request.input)),
    signal: request.signal,
  });

  if (!response.ok) {
    throw new CratestackRpcError(response.status, await readErrorBody(response, request.codec));
  }

  const contentType = response.headers.get("Content-Type") ?? "";
  if (matchesContentType(contentType, request.codec.contentType)) {
    // Server picked the configured codec — body is a single array of
    // `O`, byte-identical to `stream()`'s pre-#277 buffered behavior.
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.length === 0) {
      return;
    }
    const items = request.codec.decode(bytes) as unknown[];
    for (const item of items) {
      yield { kind: "output", output: item };
    }
    return;
  }

  if (!matchesContentType(contentType, CBOR_SEQ_CONTENT_TYPE) || response.body === null) {
    throw new CratestackRpcTransportError(
      `streaming response had unsupported Content-Type "${contentType}"`,
    );
  }

  const scanner = new CborSeqBoundaryScanner();
  const reader = response.body.getReader();
  try {
    for (;;) {
      let chunk: ReadableStreamReadResult<Uint8Array>;
      try {
        chunk = await reader.read();
      } catch (error) {
        throw new CratestackRpcTransportError(
          `network error while reading ${CBOR_SEQ_CONTENT_TYPE} stream: ${(error as Error).message}`,
        );
      }
      if (chunk.done) {
        break;
      }
      let items: Uint8Array[];
      try {
        items = scanner.feedChunk(chunk.value);
      } catch (error) {
        throw new CratestackRpcTransportError(
          `malformed ${CBOR_SEQ_CONTENT_TYPE} response: ${(error as Error).message}`,
        );
      }
      for (const itemBytes of items) {
        const frame = classifyCborSeqItem(itemBytes, request.codec);
        yield frame;
        if (frame.kind === "error") {
          // The sentinel is always the last item (rpc-transport.md
          // §3.3) — stop reading rather than polling a body that has
          // nothing more to say, regardless of what a buggy server
          // might send after it.
          return;
        }
      }
    }
    if (scanner.pendingLength > 0) {
      throw new CratestackRpcTransportError(
        `${CBOR_SEQ_CONTENT_TYPE} response ended with ${scanner.pendingLength} bytes buffered ` +
          "(truncated final item — the connection likely dropped mid-stream)",
      );
    }
  } finally {
    reader.releaseLock();
  }
};