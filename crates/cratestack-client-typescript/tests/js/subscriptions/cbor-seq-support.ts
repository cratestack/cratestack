// Shared by the `cbor-seq*.test.ts` files (cratestack#1001): a `fetch` that
// answers with `application/cbor-seq` bytes written by `@cratestack/cbor`,
// the codec of the Rust server's frames, and a client wired to it.
import { createCborCodec } from "@cratestack/cbor";
import { CratestackClientClient as CratestackClient } from "./src/client.js";

export const codec = await createCborCodec();

/** One frame as a CBOR item. */
export function frame(value: object): Uint8Array {
  return codec.encode(value) as Uint8Array;
}

export function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

export const KEEPALIVE = new Uint8Array([0xf6]);

export const event = {
  event_id: "e1",
  model: "Note",
  operation: "Created",
  occurred_at: "2026-11-01T00:00:00Z",
  data: { id: 1, body: "hi", amount: "1.50" },
};

export interface Served {
  fetch: typeof fetch;
  calls: { url: string; accept: string | null }[];
  state: { cancelled: boolean };
}

/** A `fetch` answering with `chunks`, then closing, or left open (`idle`). */
export function served(
  chunks: Uint8Array[],
  tail: "close" | "idle" = "close",
  contentType = "application/cbor-seq",
): Served {
  const calls: Served["calls"] = [];
  const state = { cancelled: false };
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), accept: new Headers(init?.headers).get("accept") });
    let i = 0;
    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (i < chunks.length) controller.enqueue(chunks[i++]!);
        else if (tail === "close") controller.close();
      },
      cancel() {
        state.cancelled = true;
      },
    });
    return new Response(body, { headers: { "content-type": contentType } });
  }) as typeof fetch;
  return { fetch: fetchFn, calls, state };
}

export const client = (fetch: typeof globalThis.fetch, maxEventBytes?: number) =>
  new CratestackClient("http://api.test", {
    fetch,
    subscriptions: maxEventBytes === undefined ? { transport: "cbor-seq" } : { transport: "cbor-seq", maxEventBytes },
  });

export async function collect(iterable: AsyncIterable<unknown>): Promise<{ items: unknown[]; error: unknown }> {
  const items: unknown[] = [];
  try {
    for await (const item of iterable) items.push(item);
  } catch (error) {
    return { items, error };
  }
  return { items, error: undefined };
}
