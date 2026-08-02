import type { RpcLinkRequest } from "@cratestack/ts-types";
import type { BatchLinkOptions, EffectiveConfig, QueueEntry } from "./types.js";

// Carried per *frame* in the `/rpc/batch` payload (`RpcRequest.idem`),
// not per request — the generated runtime writes the caller's
// `idempotencyKey` into BOTH the per-call `Idempotency-Key` header and
// the frame, so including it in the signature would put every distinct
// key in its own partition and defeat batching entirely for exactly the
// calls the dedup feature exists to serve. See
// `rpc-runtime.ts.j2`'s `call()`.
const FRAME_LEVEL_HEADERS = new Set(["idempotency-key"]);

// Field separators that cannot appear in a header name or value, so
// distinct configs cannot collide into one signature string.
const FIELD = "\u0000";
const ENTRY = "\u0001";

/** Resolves the transport config a request would actually be sent with,
 *  applying any link-level overrides on top of the request's own. */
export function effectiveConfig(
  request: RpcLinkRequest,
  options: BatchLinkOptions,
): EffectiveConfig {
  const headers = new Headers(request.headers);
  if (options.headers) {
    new Headers(options.headers).forEach((value, key) => headers.set(key, value));
  }
  return {
    headers,
    fetchFn: options.fetchFn ?? request.fetchFn,
    codec: options.codec ?? request.codec,
    batchUrl: request.urls.batch(),
  };
}

/** A stable key identifying which queued calls may share one
 *  `/rpc/batch` request. Two calls batch together only when every part
 *  of their transport envelope matches: headers, `fetch`, codec, and
 *  the resolved batch URL.
 *
 *  The URL is included because nothing stops one `createBatchLink()`
 *  instance from being passed to two runtimes pointing at different
 *  origins; without it, calls to one service would be merged into a
 *  request sent to the other. */
export function batchSignature(config: EffectiveConfig): string {
  return [
    refId(config.fetchFn),
    refId(config.codec),
    config.batchUrl,
    headerSignature(config.headers),
  ].join(FIELD);
}

function headerSignature(headers: Headers): string {
  const parts: string[] = [];
  // `.forEach()` rather than `.entries()`/`for...of`: it is the one
  // iteration style declared consistently across the DOM `Headers` lib
  // type and the Node/undici one.
  headers.forEach((value, key) => {
    const name = key.toLowerCase();
    if (!FRAME_LEVEL_HEADERS.has(name)) {
      parts.push(`${name}:${value}`);
    }
  });
  // `Headers` iteration order is not guaranteed stable across
  // implementations — sort so two identical header sets always produce
  // one signature instead of needlessly splitting a batch.
  parts.sort();
  return parts.join(ENTRY);
}

// Identity (not structural) comparison for `fetch`/codec: two distinct
// codec objects with equal `contentType` may still encode differently,
// so only the same reference is safe to batch together.
const refIds = new WeakMap<object, number>();
let nextRefId = 0;

function refId(ref: object): string {
  let id = refIds.get(ref);
  if (id === undefined) {
    id = nextRefId++;
    refIds.set(ref, id);
  }
  return String(id);
}

/** Splits a flush into partitions that may each be sent as one
 *  `/rpc/batch` request. Insertion order is preserved within every
 *  partition, and partitions are returned in first-seen order, so a
 *  flush whose calls all share a config behaves exactly as it did
 *  before partitioning existed. */
export function partition(
  entries: QueueEntry[],
  options: BatchLinkOptions,
): { config: EffectiveConfig; entries: QueueEntry[] }[] {
  const partitions: { config: EffectiveConfig; entries: QueueEntry[] }[] = [];
  const indexBySignature = new Map<string, number>();

  for (const entry of entries) {
    const config = effectiveConfig(entry.request, options);
    const signature = batchSignature(config);
    const existing = indexBySignature.get(signature);
    if (existing === undefined) {
      indexBySignature.set(signature, partitions.length);
      partitions.push({ config, entries: [entry] });
    } else {
      partitions[existing]!.entries.push(entry);
    }
  }

  return partitions;
}
