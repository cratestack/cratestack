import { describe, expect, it } from "vitest";
import { errorStatus } from "../src/correlate.js";

/** A `/rpc/batch` response is always HTTP 200; the per-frame status a
 *  caller sees is synthesized from the frame's `code`. So an unmapped
 *  code is not cosmetic — it silently becomes a 500, and 500 is exactly
 *  the status that tells a client's backoff logic "this call is broken"
 *  rather than "retry after a delay". */
describe("errorStatus", () => {
  it("maps a throttle to 429, not to a synthetic 500", () => {
    expect(errorStatus("resource_exhausted")).toBe(429);
  });

  it("maps an unreachable dependency to 503", () => {
    expect(errorStatus("unavailable")).toBe(503);
  });

  it("maps a deadline overrun to 504", () => {
    expect(errorStatus("deadline_exceeded")).toBe(504);
  });

  // `canceled` has no agreed HTTP status; it falls through to 500 with
  // the code string intact rather than inventing a number. Asserted so
  // the omission stays deliberate rather than looking like an oversight.
  it("leaves canceled on the 500 fallback, by design", () => {
    expect(errorStatus("canceled")).toBe(500);
  });

  it("still maps the codes the dispatcher emits", () => {
    expect(errorStatus("invalid_argument")).toBe(400);
    expect(errorStatus("failed_precondition")).toBe(400);
    expect(errorStatus("unauthenticated")).toBe(401);
    expect(errorStatus("permission_denied")).toBe(403);
    expect(errorStatus("not_found")).toBe(404);
    expect(errorStatus("conflict")).toBe(409);
    expect(errorStatus("internal")).toBe(500);
  });

  it("falls back to 500 for a code it has never heard of", () => {
    expect(errorStatus("something_the_server_added_later")).toBe(500);
  });
});
