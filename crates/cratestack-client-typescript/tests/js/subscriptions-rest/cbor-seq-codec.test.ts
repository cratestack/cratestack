// The `"cbor-seq"` transport's use of `@cratestack/cbor` (cratestack#1001):
// the codec is loaded before the request, so a missing or broken one never
// leaves a response body open, and a decoder's non-map output (a `Date`, a
// tagged value) is a frame error, not a frame. `@cratestack/cbor` is replaced
// by a module of the test's making, so each test loads a fresh client. REST
// package only: an RPC package imports `@cratestack/cbor` statically.
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";

type Decode = (bytes: Uint8Array) => unknown;

/** A client over a `fetch` that records its calls and the body it opened. */
async function load(mock: () => unknown, chunks: Uint8Array[] = []) {
  vi.resetModules();
  vi.doMock("@cratestack/cbor", mock);
  const { CratestackClientClient } = await import("./src/client.js");
  const { CratestackSubscriptionError } = await import("./src/subscriptions.js");
  const state = { calls: 0, cancelled: false };
  const fetchFn = (async () => {
    state.calls += 1;
    let i = 0;
    // Closed after its chunks, so a reader that wrongly goes on fails an
    // assertion rather than the test timeout. `highWaterMark: 0` keeps `pull`
    // to one call per read, so the close is not eager and a `cancel` of the
    // still-open stream is observable.
    const body = new ReadableStream<Uint8Array>(
      {
        pull(controller) {
          if (i < chunks.length) controller.enqueue(chunks[i++]!);
          else controller.close();
        },
        cancel() {
          state.cancelled = true;
        },
      },
      { highWaterMark: 0 },
    );
    return new Response(body, { headers: { "content-type": "application/cbor-seq" } });
  }) as typeof fetch;
  const client = new CratestackClientClient("http://api.test", {
    fetch: fetchFn,
    subscriptions: { transport: "cbor-seq" },
  });
  return { client, state, CratestackSubscriptionError };
}

async function failure(iterable: AsyncIterable<unknown>): Promise<{ items: unknown[]; error: unknown }> {
  const items: unknown[] = [];
  try {
    for await (const item of iterable) items.push(item);
  } catch (error) {
    return { items, error };
  }
  return { items, error: undefined };
}

/** Two empty CBOR maps: two frames, whatever the mocked decoder makes of them. */
const TWO_ITEMS = new Uint8Array([0xa0, 0xa0]);

afterEach(() => vi.doUnmock("@cratestack/cbor"));

describe("cbor-seq codec", () => {
  it("loads the codec before the request: a missing package never calls fetch", async () => {
    const { client, state, CratestackSubscriptionError } = await load(() => {
      throw new Error("Cannot find package '@cratestack/cbor'");
    });
    const { items, error } = await failure(client.seqNotes.subscribe());
    expect(items).toEqual([]);
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as { code: string }).code).toBe("codec_unavailable");
    expect(state.calls).toBe(0);
  });

  it("loads the codec before the request: a package without createCborCodec never calls fetch", async () => {
    const { client, state, CratestackSubscriptionError } = await load(() => ({ createCborCodec: undefined }));
    const { error } = await failure(client.seqNotes.subscribe());
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as { code: string }).code).toBe("codec_unavailable");
    expect(state.calls).toBe(0);
  });

  it("refuses a decoded frame that is not a plain map, and cancels the body", async () => {
    class Tagged {
      constructor(
        readonly tag: number,
        readonly value: unknown,
      ) {}
    }
    const outputs: unknown[] = [new Date(0), new Tagged(1, 0), new Map([["ready", {}]]), null, "x", [1]];
    for (const output of outputs) {
      // `ready` first, so the bad item is the second frame.
      let calls = 0;
      const decode: Decode = () => (calls++ === 0 ? { id: 1, ready: {} } : output);
      const { client, state, CratestackSubscriptionError } = await load(
        () => ({ createCborCodec: async () => ({ decode }) }),
        [TWO_ITEMS],
      );
      const { items, error } = await failure(client.seqNotes.subscribe());
      expect(items, String(output)).toEqual([]);
      expect(error, String(output)).toBeInstanceOf(CratestackSubscriptionError);
      expect((error as { code: string }).code).toBe("invalid_response");
      expect(state.cancelled).toBe(true);
    }
  });

  it("accepts a plain map made in another realm (a vm or Jest context)", async () => {
    // `Object.getPrototypeOf(frame) !== Object.prototype` here, as when a
    // native addon is loaded by the outer `require` of a sandboxed test.
    const frames: unknown[] = [runInNewContext("({id: 1, ready: {}})"), runInNewContext("({id: 2, end: {}})")];
    expect(Object.getPrototypeOf(frames[0])).not.toBe(Object.prototype);
    let calls = 0;
    const decode: Decode = () => frames[calls++];
    const { client } = await load(() => ({ createCborCodec: async () => ({ decode }) }), [TWO_ITEMS]);
    const { items, error } = await failure(client.seqNotes.subscribe());
    expect(error).toBeUndefined();
    expect(items).toEqual([]);
  });
});
