// What the error UI says about a refusal, keyed on its code (cratestack#1245):
// the `error` member an `AuthRefusal` adds (RFC 9470's
// `insufficient_user_authentication` for a step-up) when there is one, else
// the envelope's `code` (`FORBIDDEN`, `UNAUTHORIZED`, ...). Both come typed
// from the generated `CratestackHttpError`.
import { CratestackHttpError, INSUFFICIENT_USER_AUTHENTICATION } from "react-vite-swr-client/swr";

/** The refusal's code; `undefined` for an error that is not a server answer. */
export function refusalCode(error: unknown): string | undefined {
  if (!(error instanceof CratestackHttpError)) {
    return undefined;
  }
  const member = error.extra.error;
  return typeof member === "string" ? member : error.code;
}

/** The banner's sentence for a code. */
export function refusalText(code: string | undefined): string {
  switch (code) {
    case INSUFFICIENT_USER_AUTHENTICATION:
      return "This needs a fresh sign-in with your security key.";
    case "FORBIDDEN":
      return "You are not allowed to do this.";
    case "UNAUTHORIZED":
      return "Your session has ended. Sign in again.";
    case undefined:
      return "Something went wrong. Try again.";
    default:
      return `The server refused this (${code}).`;
  }
}
