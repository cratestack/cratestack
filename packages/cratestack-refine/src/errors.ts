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

/** The `message`/`statusCode` refine needs, derived from whatever was
 *  thrown. A `412 Precondition Failed` — the response an `If-Match`-guarded
 *  update/delete returns on a stale version — is surfaced as a
 *  distinguishable conflict with a human-readable message rather than
 *  folded into a generic failure; this is the one piece of information a
 *  refine UI needs to tell "your edit was rejected because someone else
 *  changed this record first" apart from every other kind of failure. */
function describe(error: unknown): { message: string; statusCode: number } {
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

/** Annotates the thrown value in place with the fields refine reads,
 *  and returns it — so its prototype, `name`, `cause`, and every own
 *  property survive the trip. `undefined` when that is not possible
 *  (a thrown primitive, a frozen or sealed object, a class exposing
 *  `message` as a getter with no setter), leaving the caller to fall
 *  back to a plain object.
 *
 *  Mutating rather than copying is deliberate: an `Object.create`-based
 *  clone preserves the prototype but silently drops private class fields
 *  (`#foo`), which is precisely the state a typed error's own methods
 *  read. There is no way to copy a class instance faithfully from
 *  outside the class. */
function annotateInPlace(
  error: unknown,
  message: string,
  statusCode: number,
): HttpError | undefined {
  if (typeof error !== "object" || error === null || !Object.isExtensible(error)) {
    return undefined;
  }
  try {
    const target = error as Record<string, unknown>;
    target.statusCode = statusCode;
    target.message = message;
    if (target.message !== message || target.statusCode !== statusCode) {
      // A setter that ignored the assignment leaves the object failing
      // `HttpError`'s contract. (A non-writable data property throws
      // instead — modules are strict mode — and lands in the `catch`.)
      // Better a plain object than a lie.
      return undefined;
    }
    return error as HttpError;
  } catch {
    return undefined;
  }
}

/** Converts a thrown error from a generated model API call into refine's
 *  `HttpError`.
 *
 *  cratestack#786: this **preserves the thrown value** rather than
 *  flattening it into a bare object literal. It used to return
 *  `{ message, statusCode }` for anything that was not a
 *  `CratestackHttpError`, discarding the value's class, `name`, `cause`
 *  and every own property — so a consumer throwing a typed error from a
 *  custom transport (a `DeviceNotEnrolledError` raised before the request
 *  ever leaves the browser, say) and classifying it with `instanceof` got
 *  correct behaviour on list screens, where nothing wrapped, and silently
 *  wrong behaviour on every detail/create/edit screen, where this ran.
 *  The only workaround was string-matching the one field that survived.
 *
 *  Whenever the thrown value is an ordinary mutable object, the returned
 *  value **is** that object, with `message` and `statusCode` set on it —
 *  `instanceof` keeps working, and so does reading any other field the
 *  thrower put there. Otherwise (a thrown primitive, a frozen object) the
 *  original is attached as `cause`, which is the standard place to look
 *  for it.
 *
 *  Note that `message` is normalized either way: a 412 gets the conflict
 *  message above, and a `CratestackHttpError` gets its envelope's
 *  `payload.message` promoted. That is the field refine renders. The
 *  original stays readable on the same object (`status`, `payload`,
 *  `response`), since it *is* the same object. */
export function toRefineError(error: unknown): HttpError {
  const { message, statusCode } = describe(error);
  return annotateInPlace(error, message, statusCode) ?? { message, statusCode, cause: error };
}
