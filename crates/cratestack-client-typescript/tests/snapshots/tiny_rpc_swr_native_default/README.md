# tiny-rpc-swr-native-default-client

Generated CrateStack TypeScript client with a fetch transport.

```ts
import { TinyRpcSwrNativeDefaultClientClient } from "tiny-rpc-swr-native-default-client";

const client = new TinyRpcSwrNativeDefaultClientClient("https://api.example.com");
```

The generated client uses `/api` as its API base path by default.

## Runtime Setup

```ts
const client = new TinyRpcSwrNativeDefaultClientClient("https://api.example.com", {
  basePath: "/api",
  headers: async () => ({
    authorization: `Bearer ${await tokenStore.getAccessToken()}`,
    "x-request-id": crypto.randomUUID(),
  }),
});
```

Per-call headers are also supported:

```ts
const headers = {
  authorization: `Bearer ${accessToken}`,
  "idempotency-key": idempotencyKey,
};
```

## Models

- `client.widgets`

### List

```ts
const pageOrItems = await client.widgets.list({
  query: {
    fields: ["id"],
    include: [],
    includeFields: {},
    limit: 20,
    offset: 0,
    sort: ["-id"],
  },
});
```

### Detail

```ts
const item = await client.widgets.get(id, {
  query: {
    fields: ["id"],
  },
  headers,
});
```

### Create, Update, Delete

```ts
const created = await client.widgets.create(input, { headers });
const updated = await client.widgets.update(created.id, patch, { headers });
await client.widgets.delete(updated.id, { headers });
```


## Procedures

- `client.procedures.echoName`

```ts
const result = await client.procedures.echoName(args, {
  headers,
});
```

## React Native

React Native app runtimes provide `fetch`; pass the mobile API origin and keep auth token lookup in your app layer.

```ts
export function createClient(accessToken: string | null) {
  return new TinyRpcSwrNativeDefaultClientClient(process.env.EXPO_PUBLIC_API_ORIGIN!, {
    basePath: "/api",
    headers: {
      ...(accessToken ? { authorization: `Bearer ${accessToken}` } : {}),
      "x-client": "sample-mobile",
    },
  });
}
```

## Decimal Fields

A `Decimal`-typed schema field is generated as a real `decimal.js`-backed `Decimal`
(cratestack#498), not a `string`:

```ts
import { Decimal } from "tiny-rpc-swr-native-default-client";
const item = await client.widgets.get(id);
console.log(item.amountField.toString()); // e.g. "0.0000001", never scientific notation
const total = item.amountField.plus(item.taxField); // real arbitrary-precision arithmetic
```

**Migration from pre-#498 generated clients:** code that treated a `Decimal` field as a
`string` (string concatenation, `Number()`/`parseFloat`, `===` comparison) needs to switch
to `Decimal`'s own API — `new Decimal(input)` to construct one, `.toString()` to format,
`.plus()`/`.minus()`/`.times()`/`.div()`/`.cmp()`/`.equals()` for arithmetic and comparison.
`DecimalFilter`'s `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`in` fields need the same treatment when
building a `Where`/`FindMany` argument by hand — encoding a `Decimal` still works with
`JSON.stringify` alone (no extra glue needed), but decoding one from raw JSON does not.

This parses correctly regardless of which `Decimal` backend built the server
(`decimal-rust-decimal` or `decimal-bigdecimal` — see `cratestack-core`'s README): both
plain positional notation (`"0.0000001"`) and the scientific notation `bigdecimal` emits
past `rust_decimal`'s ~28-29 significant-digit capacity (`"1E-7"`) decode to the identical
value, and this package always re-encodes in plain notation.
## SWR (file-per-model layout + hooks)

This package also carries the `swr` layout under `src/swr/` — one file per model (types
plus plain, framework-free `async` functions), a sibling `.hooks.ts` per model/procedures
file with `useSWR`/`useSWRMutation` hooks wrapping those functions, and a shared
`swrKeys` cache-key factory. It coexists with the layout documented above; nothing here
changes what's exported from the package root.

```ts
import { CratestackRuntime } from "tiny-rpc-swr-native-default-client/swr";
import { useWidget } from "tiny-rpc-swr-native-default-client/swr/models/widget.hooks";
```

Import exactly the functions/hooks you call. `swr`/React are only required if you import
a `.hooks` module — plain functions (`tiny-rpc-swr-native-default-client/swr/models/<model>`) need
nothing but the runtime. See `tiny-rpc-swr-native-default-client/swr`'s own module for the full model
list, and `tiny-rpc-swr-native-default-client/swr/procedures`/`/swr/procedures.hooks` for procedures.

**One `CratestackRuntime` per surface.** The root and `/swr` each construct their own
`CratestackRuntime` class from the same template, so the two are structurally identical
but *not* interchangeable at the type level (private fields make them nominally distinct
classes) — a runtime built via `new TinyRpcSwrNativeDefaultClientClient(...)`'s constructor or the
root's own `CratestackRuntime` will not type-check against a `/swr` function or hook, and
vice versa. Construct the runtime from whichever surface you're calling into, and keep
that choice consistent within one component/module.