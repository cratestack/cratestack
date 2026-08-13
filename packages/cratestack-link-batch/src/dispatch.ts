import type { RpcLinkRequest, RpcRequest, RpcResponseFrame } from "@cratestack/ts-types";
import { groupByDedupeKey, resolveGroups } from "./correlate.js";
import type { EffectiveConfig, QueueEntry } from "./types.js";

/** Sends one partition as a single `POST /rpc/batch` and settles every
 *  caller in it. A partition's entries share `config` by construction
 *  (see `./signature`), so nothing here is inherited from an arbitrary
 *  "first" call — the pre-partitioning behavior this replaces. */
export async function dispatchPartition(
  entries: QueueEntry[],
  config: EffectiveConfig,
  dedupe: (request: RpcLinkRequest) => string | null,
): Promise<void> {
  const groups = groupByDedupeKey(entries, dedupe);

  const requests: RpcRequest[] = groups.map((group, id) => {
    const request = group.entries[0]!.request;
    const frame: RpcRequest = { id, op: request.opId, input: request.input };
    if (request.idempotencyKey !== undefined) {
      frame.idem = request.idempotencyKey;
    }
    return frame;
  });

  try {
    const response = await config.fetchFn(config.batchUrl, {
      method: "POST",
      headers: config.headers,
      body: config.codec.encode(requests),
      signal: null,
    });

    if (!response.ok) {
      const bytes = new Uint8Array(await response.arrayBuffer().catch(() => new ArrayBuffer(0)));
      for (const group of groups) {
        for (const entry of group.entries) {
          entry.resolve({ response: new Response(bytes, { status: response.status }) });
        }
      }
      return;
    }

    const bytes = new Uint8Array(await response.arrayBuffer());
    const frames = config.codec.decode(bytes) as RpcResponseFrame[];
    resolveGroups(groups, frames);
  } catch (error) {
    // Scoped to this partition only — a failure here never settles a
    // caller queued under a different transport config.
    for (const group of groups) {
      for (const entry of group.entries) {
        entry.reject(error);
      }
    }
  }
}
