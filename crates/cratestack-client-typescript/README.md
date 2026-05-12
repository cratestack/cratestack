# cratestack-client-typescript

TypeScript client generator for CrateStack services.

## Overview

`cratestack-client-typescript` generates a complete TypeScript package from a `.cstack` schema, including typed models, fetch-based client, and React Query hooks.

## Installation

This is a build-time dependency used by the CLI:

```bash
cratestack generate-typescript --schema schema.cstack --out ./ts_client --name my-api-client
```

## Generated Package Structure

```
my-api-client/
├── package.json
├── tsconfig.json
├── src/
│   ├── index.ts
│   ├── runtime.ts       # Fetch wrapper, codec support
│   ├── models.ts        # Generated interfaces
│   ├── client.ts        # API client
│   ├── queries.ts       # Selection builders
│   └── react-query.ts   # React Query hooks
└── README.md
```

## Generated Types

```typescript
export interface User {
  id: string;
  email: string;
  name?: string;
  createdAt: string;
}

export interface CreateUserInput {
  email: string;
  name?: string;
}

export interface UserSelect {
  id?: boolean;
  email?: boolean;
  name?: boolean;
  includePosts?: PostInclude;
}
```

## Client Usage

```typescript
import { createClient } from 'my-api-client';
import { CborCodec } from 'my-api-client/runtime';

const client = createClient({
  baseUrl: 'https://api.example.com',
  codec: new CborCodec(),
});

// List
const users = await client.users.list({ limit: 10 });

// Get with projection
const user = await client.users.getView('usr_123', {
  id: true,
  email: true,
  includePosts: { id: true, title: true },
});

// Create
const created = await client.users.create({
  email: 'user@example.com',
  name: 'Alice',
});
```

## React Query Hooks

```typescript
import { useUsersList, useUserGet } from 'my-api-client/react-query';

function UserList() {
  const { data, isLoading, error } = useUsersList({ limit: 10 });

  if (isLoading) return <div>Loading...</div>;
  if (error) return <div>Error: {error.message}</div>;

  return (
    <ul>
      {data.map(user => <li key={user.id}>{user.email}</li>)}
    </ul>
  );
}

function UserDetail({ id }: { id: string }) {
  const { data: user } = useUserGet(id, {
    id: true,
    email: true,
    includePosts: { id: true, title: true },
  });

  return <div>{user?.email}</div>;
}
```

## Codecs

```typescript
import { CborCodec, JsonCodec } from 'my-api-client/runtime';

// CBOR (recommended for production)
const client = createClient({
  baseUrl: 'https://api.example.com',
  codec: new CborCodec(),
});

// JSON (for development/debugging)
const client = createClient({
  baseUrl: 'https://api.example.com',
  codec: new JsonCodec(),
});
```

## See Also

- `cratestack-client-rust` - Rust client
- `cratestack-cli` - CLI for code generation

## License

MIT