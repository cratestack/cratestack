// `--tanstack`'s `subscriptionQueryOptions` (slice S6) under a real
// `QueryClient` and `QueryObserver`: every item lands in the cache as it
// arrives, and the subscription is aborted once the last observer leaves.
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { CratestackClientClient as CratestackClient } from "./src/client.js";
import { subscriptionQueryOptions } from "./src/react-query-subscriptions.js";

const encoder = new TextEncoder();

function frame(name: string, data: unknown): Uint8Array {
  return encoder.encode(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
}

function note(id: number) {
  return {
    event_id: `e${id}`,
    model: "Note",
    operation: "Created",
    occurred_at: "2026-11-01T00:00:00Z",
    data: { id, body: "hi", amount: "1" },
  };
}

it("writes each item into the cache and aborts when the last observer leaves", async () => {
  let aborted = false;
  // Two items, then the stream stays open until the request is aborted.
  const fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(frame("ready", { id: 1, ready: {} }));
        controller.enqueue(frame("message", { id: 2, next: note(1) }));
        controller.enqueue(frame("message", { id: 3, next: note(2) }));
        init?.signal?.addEventListener("abort", () => {
          aborted = true;
          controller.error(new DOMException("aborted", "AbortError"));
        });
      },
    });
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  }) as typeof globalThis.fetch;
  const client = new CratestackClient("http://api.test", {
    fetch,
    subscriptions: { transport: "sse" },
  });
  const queryClient = new QueryClient();
  const key = ["notes", "events"];
  const seen: number[] = [];
  const observer = new QueryObserver(
    queryClient,
    subscriptionQueryOptions(queryClient, key, (signal) => client.notes.subscribe({}, { signal }), {
      onItem: (item) => seen.push(item.data.id),
    }),
  );
  const unsubscribe = observer.subscribe(() => {});

  await vi.waitFor(() => expect(observer.getCurrentResult().data?.data.id).toBe(2));
  expect(observer.getCurrentResult().status).toBe("success");
  expect(seen).toEqual([1, 2]);
  expect(aborted).toBe(false);

  unsubscribe();
  await vi.waitFor(() => expect(aborted).toBe(true));
});
