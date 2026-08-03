# @cratestack-examples/widgets-grpc-client

Generated CrateStack TypeScript client with a fetch transport and TanStack Query hooks.

```ts
import { CratestackExamplesWidgetsGrpcClientClient } from "@cratestack-examples/widgets-grpc-client";

const client = new CratestackExamplesWidgetsGrpcClientClient("https://api.example.com");
```

The generated client uses `/` as its API base path by default.

## Runtime Setup

```ts
const client = new CratestackExamplesWidgetsGrpcClientClient("https://api.example.com", {
  basePath: "/",
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

Not supported by `transport grpc` — `cratestack-grpc`'s generated tonic service doesn't expose procedures, so there is nothing for this client to call.

## TanStack Query

```tsx
import {
  CratestackExamplesWidgetsGrpcClientClient,
  useWidgetListQuery,
  useCreateWidgetMutation,
} from "@cratestack-examples/widgets-grpc-client";

function WidgetList({ client }: { client: CratestackExamplesWidgetsGrpcClientClient }) {
  const list = useWidgetListQuery(client, {
    query: {
      fields: ["id"],
      limit: 20,
    },
    queryOptions: {
      staleTime: 30_000,
    },
  });

  const create = useCreateWidgetMutation(client);

  if (list.isPending) {
    return null;
  }
  if (list.isError) {
    return <ErrorState error={list.error} />;
  }

  return <ListView data={list.data} onCreate={(input) => create.mutate(input)} />;
}
```

## React Native

React Native app runtimes provide `fetch`; pass the mobile API origin and keep auth token lookup in your app layer.

```ts
export function createClient(accessToken: string | null) {
  return new CratestackExamplesWidgetsGrpcClientClient(process.env.EXPO_PUBLIC_API_ORIGIN!, {
    basePath: "/",
    headers: {
      ...(accessToken ? { authorization: `Bearer ${accessToken}` } : {}),
      "x-client": "sample-mobile",
    },
  });
}
```