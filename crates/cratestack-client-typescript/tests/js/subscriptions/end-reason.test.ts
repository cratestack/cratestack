// cratestack#1381: the terminal `err` frame's `details.reason`, read as
// `CratestackSubscriptionError.endReason`, and the retry rule built on it.
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { CratestackClientClient as CratestackClient } from "./src/client.js";
import { subscriptionQueryOptions } from "./src/react-query-subscriptions.js";
import {
  CratestackSubscriptionError,
  isTerminalEndReason,
  resubscribeDelayMs,
  shouldResubscribe,
  type StreamEndReason,
} from "./src/subscriptions.js";

const encoder = new TextEncoder();

const REASONS: [StreamEndReason, string, boolean, boolean][] = [
  // reason, code, terminal, resubscribed by a hook
  ["credential_expired", "unauthenticated", false, false],
  ["stream_lifetime", "unavailable", false, true],
  ["session_ended", "unauthenticated", true, false],
  ["principal_suspended", "permission_denied", true, false],
  ["device_revoked", "permission_denied", true, false],
  ["revocation_resync", "unavailable", false, true],
];

/** A `fetch` answering with a ready frame and then the terminal `err` frame. */
function endingWith(code: string, details: unknown, opens: { n: number }): typeof fetch {
  return (async () => {
    opens.n += 1;
    const err = JSON.stringify({ id: 2, err: { code, message: "ended", details } });
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('event: ready\ndata: {"id":1,"ready":{}}\n\n'));
        controller.enqueue(encoder.encode(`event: error\ndata: ${err}\n\n`));
        controller.close();
      },
    });
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  }) as typeof fetch;
}

async function endedWith(code: string, details: unknown): Promise<unknown> {
  const client = new CratestackClient("http://api.test", {
    fetch: endingWith(code, details, { n: 0 }),
    subscriptions: { transport: "sse" },
  });
  try {
    for await (const _ of client.procedures.lines({ args: { roomId: "r1" } })) {
      // never reached
    }
  } catch (error) {
    return error;
  }
  throw new Error("the stream should have ended with an error");
}

describe("endReason", () => {
  it.each(REASONS)("reads %s from the err frame", async (reason, code, terminal, resubscribed) => {
    const error = await endedWith(code, { reason });
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as CratestackSubscriptionError).endReason).toBe(reason);
    expect(isTerminalEndReason(reason)).toBe(terminal);
    expect(shouldResubscribe(error)).toBe(resubscribed);
  });

  it("reads an unknown reason, absent details and a non-object as undefined", async () => {
    for (const details of [{ reason: "from_the_future" }, undefined, null, "session_ended", { reason: 7 }]) {
      const error = (await endedWith("unavailable", details)) as CratestackSubscriptionError;
      expect(error.endReason).toBeUndefined();
      expect(error.code).toBe("unavailable");
      expect(shouldResubscribe(error)).toBe(false);
    }
    expect(isTerminalEndReason(undefined)).toBe(false);
  });

  it("backs off with full jitter, capped", () => {
    expect(resubscribeDelayMs(0, () => 0)).toBe(0);
    expect(resubscribeDelayMs(0, () => 0.999)).toBeLessThan(500);
    expect(resubscribeDelayMs(3, () => 0.5)).toBe(2000);
    expect(resubscribeDelayMs(40, () => 0.999)).toBeLessThan(30_000);
  });
});

describe("useSubscriptionQuery retry", () => {
  async function opensFor(reason: StreamEndReason, code: string): Promise<number> {
    const opens = { n: 0 };
    const client = new CratestackClient("http://api.test", {
      fetch: endingWith(code, { reason }, opens),
      subscriptions: { transport: "sse" },
    });
    const queryClient = new QueryClient();
    const options = subscriptionQueryOptions(queryClient, ["lines", reason], (signal) =>
      client.procedures.lines({ args: { roomId: "r1" } }, { signal }),
    );
    const observer = new QueryObserver(queryClient, { ...options, retryDelay: 1 });
    const unsubscribe = observer.subscribe(() => {});
    await new Promise((resolve) => setTimeout(resolve, 300));
    unsubscribe();
    return opens.n;
  }

  it("reopens stream_lifetime and revocation_resync, and nothing else", async () => {
    expect(await opensFor("stream_lifetime", "unavailable")).toBeGreaterThan(1);
    expect(await opensFor("revocation_resync", "unavailable")).toBeGreaterThan(1);
    for (const [reason, code] of [
      ["session_ended", "unauthenticated"],
      ["principal_suspended", "permission_denied"],
      ["device_revoked", "permission_denied"],
      ["credential_expired", "unauthenticated"],
    ] as const) {
      expect(await opensFor(reason, code)).toBe(1);
    }
  });

  it("never caps the retries of an end a retry fixes", () => {
    const options = subscriptionQueryOptions(new QueryClient(), ["lines"], () => {
      throw new Error("not opened");
    });
    const retry = options.retry as (failureCount: number, error: Error) => boolean;
    const ended = (reason: StreamEndReason) =>
      new CratestackSubscriptionError("unavailable", "ended", { reason });
    // A stream ending `stream_lifetime` every hour must still be retried
    // after the fifth hour: the count only grows while it delivers items.
    for (const reason of ["stream_lifetime", "revocation_resync"] as const) {
      expect(retry(0, ended(reason))).toBe(true);
      expect(retry(5, ended(reason))).toBe(true);
      expect(retry(10_000, ended(reason))).toBe(true);
    }
    expect(retry(0, ended("session_ended"))).toBe(false);
    expect(retry(0, new Error("boom"))).toBe(false);
  });
});
