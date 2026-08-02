import type { RpcErrorBody, RpcLinkRequest, RpcResponseFrame } from "@cratestack/ts-types";
import type { Group, QueueEntry } from "./types.js";

/** Collapses entries that share a non-null dedupe key into one frame.
 *  Runs *within* a partition — this is frame-level dedup, distinct from
 *  the request-level split in `./signature`. */
export function groupByDedupeKey(
  entries: QueueEntry[],
  dedupe: (request: RpcLinkRequest) => string | null,
): Group[] {
  const groups: Group[] = [];
  const indexByKey = new Map<string, number>();
  for (const entry of entries) {
    const key = dedupe(entry.request);
    if (key === null) {
      groups.push({ key: null, entries: [entry] });
      continue;
    }
    const existingIndex = indexByKey.get(key);
    if (existingIndex === undefined) {
      indexByKey.set(key, groups.length);
      groups.push({ key, entries: [entry] });
    } else {
      groups[existingIndex]!.entries.push(entry);
    }
  }
  return groups;
}

/** Fans a decoded `/rpc/batch` response back out to each queued caller. */
export function resolveGroups(groups: Group[], frames: RpcResponseFrame[]): void {
  // Correlate by `id`, not array position — the server contract
  // guarantees order (docs/design/rpc-transport.md §3.2), but matching
  // by id is strictly more robust and is the pattern the repo's own
  // Rust client-side batch debouncer already establishes
  // (examples/rpc-batch-debounce/src/lib.rs's `responders` map).
  const frameById = new Map(frames.map((frame) => [frame.id, frame]));
  groups.forEach((group, id) => {
    const frame = frameById.get(id);
    if (!frame) {
      const error = new Error(`batch response is missing frame id ${id}`);
      for (const entry of group.entries) {
        entry.reject(error);
      }
      return;
    }

    const hasError = frame.error !== undefined;
    for (const entry of group.entries) {
      if (!hasError && frame.output === undefined) {
        // Mirrors the unary path's `response.status === 204` shortcut
        // (void-returning calls resolve `undefined`, not a decoded null).
        entry.resolve({ response: new Response(null, { status: 204 }) });
        continue;
      }
      const body: RpcErrorBody | unknown = hasError ? frame.error : frame.output;
      // Re-encoded with the *caller's own* codec, not the batch's — the
      // caller's generated runtime decodes this synthetic response with
      // the codec it was constructed with, which an `options.codec`
      // override on this link must not change.
      entry.resolve({
        response: new Response(entry.request.codec.encode(body), {
          status: hasError ? errorStatus(frame.error!.code) : 200,
        }),
      });
    }
  });
}

export function errorStatus(code: string): number {
  switch (code) {
    case "invalid_argument":
    case "failed_precondition":
      return 400;
    case "unauthenticated":
      return 401;
    case "permission_denied":
      return 403;
    case "not_found":
      return 404;
    case "conflict":
      return 409;
    default:
      return 500;
  }
}
