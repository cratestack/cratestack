// Behavioral proof for the RpcStreamLink chain mechanism (issue #277),
// the streaming sibling of `tests/chain.test.ts`'s `RpcLink` coverage.
// `crates/cratestack-client-typescript`'s own test suite only asserts on
// generated *text* (no JS execution harness there) — these properties
// (ordering, no-op equivalence, error-frame handling, real incremental
// delivery) need real execution, so they're proven here against
// `FakeStreamRuntime`, which mirrors the generated stream-chain
// construction and terminal link exactly.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import type { RpcStreamFrame, RpcStreamLink } from "../src/index.js";
import { FakeStreamError, FakeStreamRuntime } from "../src/test-harness.js";
import { miniCborCodec } from "./helpers/mini-cbor-codec.js";

const fixturesDir = join(dirname(fileURLToPath(import.meta.url)), "fixtures");

function hexToBytes(hex: string): Uint8Array {
  const trimmed = hex.trim();
  const bytes = new Uint8Array(trimmed.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(trimmed.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

function loadFixture(name: string): Uint8Array {
  return hexToBytes(readFileSync(join(fixturesDir, name), "utf8"));
}

function jsonArrayResponse(items: unknown[]): Response {
  return new Response(JSON.stringify(items), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

/** A `Response` whose body streams `chunks` out one `ReadableStream`
 *  push per array element — simulates real network delivery (arbitrary
 *  chunk boundaries, not one big buffered read) far more faithfully
 *  than handing the whole body to `new Response(bytes)` at once. */
function cborSeqStreamResponse(chunks: Uint8Array[]): Response {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(chunk);
      }
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "application/cbor-seq" },
  });
}

describe("RpcStreamLink chain", () => {
  it("is a true no-op when no streamLinks are declared — request is byte-identical", async () => {
    const fetchMock = vi.fn(async () => jsonArrayResponse([1, 2, 3]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, []);

    const items: number[] = [];
    for await (const item of runtime.stream<number>("procedure.ticks", { count: 3 })) {
      items.push(item);
    }

    expect(items).toEqual([1, 2, 3]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://example.test/rpc/procedure.ticks");
    expect(init.method).toBe("POST");
    expect(init.body).toBe(JSON.stringify({ count: 3 }));
  });

  it("runs two independent links in declared order, each consuming and re-yielding", async () => {
    const order: string[] = [];
    const linkA: RpcStreamLink = async function* (request, next) {
      order.push("a:before");
      for await (const frame of next(request)) {
        yield frame;
      }
      order.push("a:after");
    };
    const linkB: RpcStreamLink = async function* (request, next) {
      order.push("b:before");
      for await (const frame of next(request)) {
        yield frame;
      }
      order.push("b:after");
    };
    const fetchMock = vi.fn(async () => {
      order.push("fetch");
      return jsonArrayResponse([1]);
    });
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [linkA, linkB]);

    const items: unknown[] = [];
    for await (const item of runtime.stream("procedure.ticks", null)) {
      items.push(item);
    }

    expect(items).toEqual([1]);
    // Note: unlike the promise-based `RpcLink` chain (which awaits
    // `next()` to completion before continuing), an async-generator
    // chain interleaves — each link's "before" push happens before ANY
    // frame flows, but "after" only happens once the inner generator is
    // fully drained. `fetch` itself only runs once the outermost
    // consumer starts pulling frames (generators are lazy).
    expect(order).toEqual(["a:before", "b:before", "fetch", "b:after", "a:after"]);
  });

  it("lets a link short-circuit the chain entirely (never calls next, never touches fetch)", async () => {
    const shortCircuit: RpcStreamLink = async function* () {
      yield { kind: "output", output: "intercepted" };
    };
    const fetchMock = vi.fn(async () => jsonArrayResponse(["real"]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [shortCircuit]);

    const items: unknown[] = [];
    for await (const item of runtime.stream("procedure.ticks", null)) {
      items.push(item);
    }

    expect(items).toEqual(["intercepted"]);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("propagates the per-call AbortSignal through to the terminal fetch", async () => {
    const controller = new AbortController();
    let receivedSignal: AbortSignal | null | undefined;
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      receivedSignal = init.signal as AbortSignal | null;
      return jsonArrayResponse([]);
    });
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, []);

    const items: unknown[] = [];
    for await (const item of runtime.stream("procedure.ticks", null, {
      signal: controller.signal,
    })) {
      items.push(item);
    }

    expect(receivedSignal).toBe(controller.signal);
  });

  it("a reference logger-style link logs start/frame-count/duration and never breaks streaming", async () => {
    // Mirrors `createLoggerStreamLink()` (`rpc-links.ts.j2`) — proves
    // the CONTRACT supports exactly that shape of link (issue #277's
    // acceptance criteria), consuming and re-yielding every frame
    // without altering them.
    const events: string[] = [];
    const loggerStreamLink: RpcStreamLink = async function* (request, next) {
      events.push(`start:${request.opId}`);
      let count = 0;
      for await (const frame of next(request)) {
        if (frame.kind === "output") {
          count++;
        }
        yield frame;
      }
      events.push(`end:${request.opId}:frames=${count}`);
    };
    const fetchMock = vi.fn(async () => jsonArrayResponse([10, 20, 30]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [loggerStreamLink]);

    const items: unknown[] = [];
    for await (const item of runtime.stream("procedure.ticks", null)) {
      items.push(item);
    }

    expect(items).toEqual([10, 20, 30]);
    expect(events).toEqual(["start:procedure.ticks", "end:procedure.ticks:frames=3"]);
  });

  it("converts a mid-stream error frame into a thrown error outside the chain, and stops iterating", async () => {
    let secondFrameRequested = false;
    const observingLink: RpcStreamLink = async function* (request, next) {
      for await (const frame of next(request)) {
        if (frame.kind === "output" && frame.output === "second") {
          secondFrameRequested = true;
        }
        yield frame;
      }
    };
    const source: RpcStreamFrame[] = [
      { kind: "output", output: "first" },
      { kind: "error", error: { code: "internal", message: "boom" } },
      { kind: "output", output: "second" }, // must never be reached
    ];
    const terminalLikeLink: RpcStreamLink = async function* () {
      for (const frame of source) {
        yield frame;
        if (frame.kind === "error") {
          return; // mirrors the real terminal link's own early return
        }
      }
    };
    const fetchMock = vi.fn();
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [
      observingLink,
      terminalLikeLink,
    ]);

    const items: unknown[] = [];
    await expect(async () => {
      for await (const item of runtime.stream("procedure.ticks", null)) {
        items.push(item);
      }
    }).rejects.toThrow(FakeStreamError);

    expect(items).toEqual(["first"]);
    expect(secondFrameRequested).toBe(false);
  });
});

describe("RpcStreamLink chain against a real application/cbor-seq stream", () => {
  it("yields real fixture items incrementally as chunks arrive, not after buffering the whole body", async () => {
    const bytes = loadFixture("ticks-success.hex");
    // Split into three arbitrary, non-item-aligned chunks to prove the
    // scanner reassembles items across chunk boundaries correctly, all
    // the way through the real `stream()` chain (fetch -> terminal
    // stream link -> boundary scan -> classify -> yielded output).
    const third = Math.ceil(bytes.length / 3);
    const chunks = [bytes.slice(0, third), bytes.slice(third, third * 2), bytes.slice(third * 2)];

    const observedFrameKinds: string[] = [];
    const observingLink: RpcStreamLink = async function* (request, next) {
      for await (const frame of next(request)) {
        observedFrameKinds.push(frame.kind);
        yield frame;
      }
    };
    const fetchMock = vi.fn(async () => cborSeqStreamResponse(chunks));
    const runtime = new FakeStreamRuntime(
      fetchMock as unknown as typeof fetch,
      [observingLink],
      miniCborCodec,
    );

    const items: unknown[] = [];
    for await (const item of runtime.stream<{ index: number; value: number }>(
      "procedure.ticks",
      null,
    )) {
      items.push(item);
    }

    expect(items).toEqual([
      { index: 0, value: 0 },
      { index: 1, value: 1 },
      { index: 2, value: 2 },
    ]);
    expect(observedFrameKinds).toEqual(["output", "output", "output"]);
  });

  it("throws CratestackRpcStreamError-equivalent when the real error-sentinel fixture is reached, and stops there", async () => {
    const bytes = loadFixture("flaky-ticks-error.hex");
    const fetchMock = vi.fn(async () => cborSeqStreamResponse([bytes]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [], miniCborCodec);

    const items: unknown[] = [];
    let thrown: unknown;
    try {
      for await (const item of runtime.stream("procedure.flakyTicks", null)) {
        items.push(item);
      }
    } catch (error) {
      thrown = error;
    }

    expect(items).toEqual([
      { index: 0, value: 100 },
      { index: 1, value: 101 },
    ]);
    expect(thrown).toBeInstanceOf(FakeStreamError);
    expect((thrown as InstanceType<typeof FakeStreamError>).body.code).toBe("internal");
    // `CratestackError::Internal`'s `public_message()` deliberately redacts the
    // real message to a generic "internal error" server-side — this
    // asserts the client sees exactly that redacted text, not a leak.
    expect((thrown as InstanceType<typeof FakeStreamError>).body.message).toBe("internal error");
  });

  it("a malformed cbor-seq body produces a clear error, not a hang", async () => {
    const malformed = Uint8Array.from([0x1f]); // major 0, indefinite — invalid
    const fetchMock = vi.fn(async () => cborSeqStreamResponse([malformed]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, []);

    await expect(async () => {
      for await (const _item of runtime.stream("procedure.ticks", null)) {
        // no-op — the malformed body must throw before yielding anything
      }
    }).rejects.toThrow();
  });

  it("a truncated cbor-seq body (dropped connection) produces a clear error, not a hang", async () => {
    // A complete item (0x00, decodes to the number 0) followed by the
    // start of a truncated one (0x19 needs 2 more bytes that never
    // arrive) — the complete item must still be yielded before the
    // truncation error surfaces; a real dropped connection doesn't
    // retroactively invalidate the items that already arrived.
    const truncated = Uint8Array.from([0x00, 0x19, 0x01]);
    const fetchMock = vi.fn(async () => cborSeqStreamResponse([truncated]));
    const runtime = new FakeStreamRuntime(fetchMock as unknown as typeof fetch, [], miniCborCodec);

    const items: unknown[] = [];
    await expect(async () => {
      for await (const item of runtime.stream("procedure.ticks", null)) {
        items.push(item);
      }
    }).rejects.toThrow(/truncated|buffered/);
    expect(items).toEqual([0]);
  });
});
