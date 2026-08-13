import type { RpcErrorBody, RpcLink } from "@cratestack/ts-types";
import type { ZodTypeAny } from "zod";

/** Maps a `request.opId` to the schema its `input` must satisfy. Ops
 *  with no entry pass through unvalidated — this link is opt-in per op,
 *  not a blanket gate. */
export type ZodValidatorSchemas = Record<string, ZodTypeAny>;

/** Validates `request.input` against a per-op `zod` schema before
 *  calling `next()`, shipped as an
 *  [`RpcLink`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-ts-types)
 *  ([issue #182](https://github.com/cratestack/cratestack/issues/182)) so it composes with
 *  `@cratestack/link-batch`/`@cratestack/link-logger` instead of requiring its own call site.
 *
 *  On success, `next()` receives the schema's *parsed* output as
 *  `input` — so `z.coerce.number()`/defaults/transforms actually take
 *  effect on the wire, not just at the type level. On failure, the
 *  chain short-circuits: `next()` is never called and the link resolves
 *  its own `Response` carrying an `RpcErrorBody` with
 *  `code: "invalid_argument"`, mirroring what the server itself would
 *  return for a rejected input (see `docs/design/rpc-transport.md`). */
export function createZodValidatorLink(schemas: ZodValidatorSchemas): RpcLink {
  return async (request, next) => {
    const schema = schemas[request.opId];
    if (!schema) {
      return next(request);
    }

    const result = schema.safeParse(request.input);
    if (result.success) {
      return next({ ...request, input: result.data });
    }

    const body: RpcErrorBody = {
      code: "invalid_argument",
      message: `input for "${request.opId}" failed validation`,
      details: result.error.flatten(),
    };
    return { response: new Response(request.codec.encode(body), { status: 400 }) };
  };
}
