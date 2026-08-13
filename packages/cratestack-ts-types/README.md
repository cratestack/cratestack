# @cratestack/ts-types

Shared TypeScript interfaces for CrateStack's generated TypeScript RPC client (`transport rpc`
schemas) and for the rest of the `@cratestack/*` npm family (`link-*`, `runtime-*`, `validator-*`,
`adapter-*`).

Pinned local copies of the wire/link contract generated into every CrateStack `transport rpc`
project by `crates/cratestack-client-typescript/templates/src/rpc-links.ts.j2` and
`rpc-runtime.ts.j2` ([issue #182](https://github.com/cratestack/cratestack/issues/182)). Kept as
plain interfaces/function types deliberately — a generated project's `CratestackRpcRuntime` is a
per-project class with no shared import path, so this package can't (and doesn't need to) import
it; TypeScript's structural typing means any object shaped like these types is assignable into a
generated client's `links` array, regardless of which project generated it.

This package has **no runtime code** — every export is an `interface` or `type`, so it compiles
away to (essentially) nothing. Everything else in the `@cratestack/*` family depends on it for
types only (`import type`), so pulling in `link-batch`, `validator-zod`, etc. never adds this
package's weight to your bundle.

## Exports

- **`.`** — `RpcLink`, `RpcLinkRequest`, `RpcLinkResponse`, `RpcLinkNext`, `CratestackRpcCodec`,
  `RpcRequest`, `RpcResponseFrame`, `RpcErrorBody`.
- **`./test-harness`** — `FakeRuntime`, a minimal in-memory runtime that mirrors
  `CratestackRpcRuntime`'s chain construction exactly, for testing `RpcLink` implementations
  against a real chain instead of a reimplementation of one. Not part of the public API surface —
  used by this package's own tests and by every other `@cratestack/*` package's test suite.

## Usage

```ts
import type { RpcLink } from "@cratestack/ts-types";

export function createMyLink(): RpcLink {
  return async (request, next) => {
    // ...
    return next(request);
  };
}
```
