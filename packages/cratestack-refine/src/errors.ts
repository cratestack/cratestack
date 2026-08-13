import type { HttpError } from "@refinedev/core";
import type { CratestackHttpErrorLike } from "./types.js";

/** Structural check for the generated client's `CratestackHttpError`
 *  (`crates/cratestack-client-typescript/templates/src/rest-runtime.ts.j2`)
 *  — `instanceof` doesn't work here because that class is regenerated
 *  per consumer into their own package, so this package has no shared
 *  class reference to check against. `status` is set unconditionally by
 *  the runtime's `!response.ok` branch regardless of codec, which makes
 *  it the more robust field to key on than pattern-matching the error
 *  envelope's `payload.code`. */
export function isCratestackHttpError(error: unknown): error is CratestackHttpErrorLike {
  return (
    typeof error === "object" &&
    error !== null &&
    "status" in error &&
    typeof (error as { status: unknown }).status === "number"
  );
}

/** Converts a thrown error from a generated model API call into refine's
 *  `HttpError`. A `412 Precondition Failed` — the response an
 *  `If-Match`-guarded update/delete returns on a stale version — is
 *  surfaced as a distinguishable conflict (`statusCode: 412` with a
 *  human-readable message) rather than folded into a generic failure;
 *  this is the one piece of information a refine UI needs to tell "your
 *  edit was rejected because someone else changed this record first"
 *  apart from every other kind of failure. */
export function toRefineError(error: unknown): HttpError {
  if (isCratestackHttpError(error)) {
    if (error.status === 412) {
      return {
        message: "This record changed since it was loaded. Reload it and try again.",
        statusCode: 412,
      };
    }
    const payload = error.payload as { message?: string } | undefined;
    return {
      message: payload?.message ?? error.message ?? "Request failed",
      statusCode: error.status,
    };
  }
  return { message: error instanceof Error ? error.message : "Unknown error", statusCode: 500 };
}
