# @cratestack/validator-zod

An [`RpcLink`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-ts-types)
([issue #182](https://github.com/cratestack/cratestack/issues/182)) for CrateStack's generated
TypeScript RPC client (`transport rpc` schemas) that validates `request.input` against a per-op
[zod](https://zod.dev/) schema before the call reaches the network — client-side validation with
the exact same schema shape your server-side procedure/model input already has, instead of hoping
the generated types alone catch a bad call at compile time.

## Usage

```ts
import { createZodValidatorLink } from "@cratestack/validator-zod";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client
import { z } from "zod";

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  links: [
    createZodValidatorLink({
      "model.Order.create": z.object({ total: z.number().positive() }),
    }),
  ],
});
```

Ops with no configured schema pass through unvalidated — this is opt-in per op, not a blanket
gate. On success, the schema's *parsed* output (defaults, `z.coerce`, transforms) becomes the
actual request `input`, not just a type-level assertion. On failure, the chain short-circuits —
the real network call never happens — and the call rejects the same way a server-side validation
failure would, with an `RpcErrorBody` carrying `code: "invalid_argument"`.
