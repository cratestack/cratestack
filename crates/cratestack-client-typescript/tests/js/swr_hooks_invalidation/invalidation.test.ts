// Real vitest + @testing-library/react proof for issue #305 AC #6
// ("Invalidation is proven by test, not asserted: a test renders a
// hook, performs a mutation, and confirms the dependent query refetches
// AND that an unrelated query does not") and AC #7 (the null-id
// conditional-fetching case). Copied verbatim (by
// `tests/swr_hooks_invalidation.rs`) alongside a `swr_hooks_invalidation`
// preset-generated package for a two-model RPC fixture, then run for
// real with `npx vitest run` — not a Rust string assertion.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { SWRConfig } from "swr";
import { CratestackRpcRuntime } from "./src/runtime";
import { useWidgets, useWidget, useCreateWidget, useUpdateWidget, useDeleteWidget } from "./src/models/widget.hooks";
import { useGadgets } from "./src/models/gadget.hooks";

afterEach(() => cleanup());

function stubRuntime() {
  const calls: string[] = [];
  const runtime = new CratestackRpcRuntime("http://example.invalid", { basePath: "/api" });
  // Overrides the instance's `call` method with a recording stub —
  // `CratestackRpcRuntime` has private fields, so a plain object
  // literal can't structurally satisfy the type; a real instance with
  // its public `call` method swapped keeps every hook/plain-function
  // call site exercised for real, only the network hop is faked.
  runtime.call = vi.fn(async (opId: string, input: unknown) => {
    calls.push(opId);
    switch (opId) {
      case "model.Widget.list":
        return [{ id: 1, name: "Widget One" }];
      case "model.Widget.get":
        return { id: (input as { id: number }).id, name: "Widget One" };
      case "model.Widget.create":
        return { id: 2, name: (input as { name: string }).name };
      case "model.Widget.update":
        return { id: (input as { id: number }).id, name: "Updated" };
      case "model.Widget.delete":
        return undefined;
      case "model.Gadget.list":
        return [{ id: 10, name: "Gadget One" }];
      default:
        throw new Error(`stub runtime: unhandled op ${opId}`);
    }
  }) as typeof runtime.call;
  return { runtime, calls };
}

// Every hook under test calls `useSWRConfig()` for its bound `mutate` —
// so hooks only observe each other's invalidation if they share the
// same SWR cache. `renderHook()` (testing-library) mounts each call
// into its OWN, separate React root, so hooks in the same test need an
// explicit shared cache — the isolated-Map-provider pattern from SWR's
// own testing docs (https://swr.vercel.app/docs/advanced/cache#reset-cache-between-test-cases).
// SWR's `initCache` (verified by reading `swr/dist/_internal`, not
// assumed) keys its global state by the *cache object* a `provider`
// function returns, calling that function fresh on every `<SWRConfig>`
// mount — so `provider: () => new Map()` would hand every mount in
// this test its own unshared cache. `cacheProvider` below closes over
// one `Map` instance and always returns that same instance, so every
// `renderHook` in one test shares it (mutations become visible to
// sibling hooks); a fresh `Map` per test (created inside each `it()`)
// keeps tests from leaking cached data into one another.
function withIsolatedCache() {
  const sharedCache = new Map();
  const cacheProvider = () => sharedCache;
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(SWRConfig, { value: { provider: cacheProvider } }, children);
  return { wrapper };
}

describe("swr hooks — invalidation (issue #305 AC #6)", () => {
  it("create invalidates the model's list but not an unrelated model's list", async () => {
    const { runtime, calls } = stubRuntime();
    const { wrapper } = withIsolatedCache();

    // Rendered and settled one at a time: two `renderHook()` roots
    // mounted back to back with no settle point in between can race in
    // this test harness (@testing-library/react + jsdom) — a detail of
    // this harness, not a claim about the generated hooks themselves.
    const list = renderHook(() => useWidgets(runtime), { wrapper });
    await waitFor(() => expect(list.result.current.data).toBeDefined());
    const other = renderHook(() => useGadgets(runtime), { wrapper });
    await waitFor(() => expect(other.result.current.data).toBeDefined());

    expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(1);
    expect(calls.filter((c) => c === "model.Gadget.list")).toHaveLength(1);

    const mutation = renderHook(() => useCreateWidget(runtime), { wrapper });
    await mutation.result.current.trigger({ name: "New Widget" });

    // Dependent query (Widget's list) refetches...
    await waitFor(() =>
      expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(2),
    );
    // ...but the unrelated model's list does not.
    expect(calls.filter((c) => c === "model.Gadget.list")).toHaveLength(1);
  });

  it("update invalidates both the list and that entity's detail", async () => {
    const { runtime, calls } = stubRuntime();
    const { wrapper } = withIsolatedCache();

    const list = renderHook(() => useWidgets(runtime), { wrapper });
    await waitFor(() => expect(list.result.current.data).toBeDefined());
    const detail = renderHook(() => useWidget(runtime, 1), { wrapper });
    await waitFor(() => expect(detail.result.current.data).toBeDefined());
    expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(1);
    expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(1);

    const mutation = renderHook(() => useUpdateWidget(runtime, 1), { wrapper });
    await mutation.result.current.trigger({ name: "Renamed" });

    await waitFor(() =>
      expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(2),
    );
    await waitFor(() =>
      expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(2),
    );
  });

  it("delete invalidates the list and drops the detail without refetching it", async () => {
    const { runtime, calls } = stubRuntime();
    const { wrapper } = withIsolatedCache();

    const list = renderHook(() => useWidgets(runtime), { wrapper });
    await waitFor(() => expect(list.result.current.data).toBeDefined());
    const detail = renderHook(() => useWidget(runtime, 1), { wrapper });
    await waitFor(() => expect(detail.result.current.data).toBeDefined());
    expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(1);
    expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(1);

    const mutation = renderHook(() => useDeleteWidget(runtime, 1), { wrapper });
    await mutation.result.current.trigger();

    await waitFor(() =>
      expect(calls.filter((c) => c === "model.Widget.list")).toHaveLength(2),
    );
    // Detail is dropped (`revalidate: false`), not refetched — the call
    // count for `model.Widget.get` must stay at 1 even after settling,
    // and the cached detail value itself is cleared.
    await waitFor(() => expect(detail.result.current.data).toBeUndefined());
    expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(1);
  });

  it("a detail hook with a null id never fires a request (conditional fetching, AC #7)", async () => {
    const { runtime, calls } = stubRuntime();
    const { wrapper } = withIsolatedCache();

    const detail = renderHook(({ id }: { id: number | null }) => useWidget(runtime, id), {
      wrapper,
      initialProps: { id: null },
    });

    // Give SWR a tick to (not) fire, then prove it stayed idle.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(0);
    expect(detail.result.current.data).toBeUndefined();
    expect(detail.result.current.isLoading).toBe(false);

    detail.rerender({ id: 1 });
    await waitFor(() => expect(detail.result.current.data).toBeDefined());
    expect(calls.filter((c) => c === "model.Widget.get")).toHaveLength(1);
  });
});
