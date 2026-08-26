# tiny-rpc-native-default-client

Generated CrateStack TypeScript client with a fetch transport.

```ts
import { TinyRpcNativeDefaultClientClient } from "tiny-rpc-native-default-client";

const client = new TinyRpcNativeDefaultClientClient("https://api.example.com");
```

The generated client uses `/api` as its API base path by default.

## Runtime Setup

```ts
const client = new TinyRpcNativeDefaultClientClient("https://api.example.com", {
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
  return new TinyRpcNativeDefaultClientClient(process.env.EXPO_PUBLIC_API_ORIGIN!, {
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
import { Decimal } from "tiny-rpc-native-default-client";
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