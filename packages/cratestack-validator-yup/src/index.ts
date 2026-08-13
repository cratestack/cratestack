import type { RpcErrorBody, RpcLink } from "@cratestack/ts-types";
import type { AnySchema } from "yup";
import { ValidationError } from "yup";

/** Maps a `request.opId` to the schema its `input` must satisfy. Ops
 *  with no entry pass through unvalidated — this link is opt-in per op,
 *  not a blanket gate. */
export type YupValidatorSchemas = Record<string, AnySchema>;

/** Validates `request.input` against a per-op `yup` schema before
 *  calling `next()`, shipped as an
 *  [`RpcLink`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-ts-types)
 *  ([issue #182](https://github.com/cratestack/cratestack/issues/182)) so it composes with
 *  `@cratestack/link-batch`/`@cratestack/link-logger` instead of requiring its own call site.
 *
 *  On success, `next()` receives the schema's *cast* output as `input`
 *  — so defaults/type coercion actually take effect on the wire, not
 *  just at the type level. On failure, the chain short-circuits:
 *  `next()` is never called and the link resolves its own `Response`
 *  carrying an `RpcErrorBody` with `code: "invalid_argument"`,
 *  mirroring what the server itself would return for a rejected input
 *  (see `docs/design/rpc-transport.md`). */
export function createYupValidatorLink(schemas: YupValidatorSchemas): RpcLink {
  return async (request, next) => {
    const schema = schemas[request.opId];
    if (!schema) {
      return next(request);
    }

    try {
      const value = await schema.validate(request.input, { abortEarly: false, stripUnknown: true });
      return next({ ...request, input: value });
    } catch (error) {
      if (!(error instanceof ValidationError)) {
        throw error;
      }
      const body: RpcErrorBody = {
        code: "invalid_argument",
        message: `input for "${request.opId}" failed validation`,
        details: error.errors,
      };
      return { response: new Response(request.codec.encode(body), { status: 400 }) };
    }
  };
}
