// The step-up flow on the generated swr layer's plain functions
// (cratestack#1245): `clearDoneTasks` against a fake `fetch` that answers
// the way the server does under `@authn(maxAge: 300, amr: ["hwk"])`, read
// from the same `DemoSession` the app's headers come from. Run with
// `pnpm --filter react-vite-swr-web test` (tsx + node:test; the client must
// be built first, which the `pretest` script does).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CratestackHttpError,
  CratestackRuntime,
  clearDoneTasks,
  retryAfterStepUp,
} from "react-vite-swr-client/swr";
import { DemoSession } from "../src/session.ts";
import { refusalCode, refusalText } from "../src/stepUp/refusal.ts";

const CHALLENGE =
  'Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300"';

/** A fake server: refuses a session that is too old or has no `hwk`. */
function fakeServer() {
  const seen: Record<string, string>[] = [];
  const fetch = async (_url: RequestInfo | URL, init?: RequestInit) => {
    const headers = Object.fromEntries(new Headers(init?.headers).entries());
    seen.push(headers);
    const age = Math.floor(Date.now() / 1000) - Number(headers["x-auth-time"]);
    const unmet = [
      ...(age > 300 ? ["max_age"] : []),
      ...((headers["x-auth-amr"] ?? "").split(" ").includes("hwk") ? [] : ["amr"]),
    ];
    if (unmet.length > 0) {
      const body = {
        code: "UNAUTHORIZED",
        message: "step-up authentication required",
        error: "insufficient_user_authentication",
        details: { max_age: 300, amr: ["hwk"], unmet },
      };
      return new Response(JSON.stringify(body), {
        status: 401,
        headers: { "content-type": "application/json", "www-authenticate": CHALLENGE },
      });
    }
    return Response.json({ removed: 2 });
  };
  return { fetch: fetch as typeof globalThis.fetch, seen };
}

function setup() {
  const session = DemoSession.stalePassword();
  const server = fakeServer();
  const runtime = new CratestackRuntime("http://server.test", {
    basePath: "/api",
    fetch: server.fetch,
    headers: () => session.headers(),
  });
  const call = () => clearDoneTasks(runtime, { args: { boardId: 1 } });
  return { session, server, call };
}

test("a stale password session steps up once, then the call succeeds", async () => {
  const { session, server, call } = setup();
  const asked: unknown[] = [];
  const result = await retryAfterStepUp(call, async (stepUp) => {
    asked.push(stepUp);
    session.stepUp();
    return true;
  });
  assert.equal(result.removed, 2);
  assert.deepEqual(asked, [
    { maxAge: 300, acrValues: [], amr: ["hwk"], unmet: ["max_age", "amr"] },
  ]);
  assert.equal(server.seen.length, 2, "one retry, no more");
  assert.equal(server.seen[1]?.["x-auth-amr"], "hwk user", "the retry carries the new session");
});

test("a declined step-up rethrows the typed refusal without a retry", async () => {
  const { server, call } = setup();
  const error = await retryAfterStepUp(call, async () => false).catch((caught) => caught);
  assert.ok(error instanceof CratestackHttpError);
  assert.equal(error.status, 401);
  assert.deepEqual(error.challenges, [CHALLENGE]);
  assert.deepEqual(error.stepUp?.unmet, ["max_age", "amr"]);
  assert.equal(refusalCode(error), "insufficient_user_authentication");
  assert.equal(server.seen.length, 1);
});

test("a re-proof that falls short is not retried a second time", async () => {
  const { session, server, call } = setup();
  const error = await retryAfterStepUp(call, async () => {
    session.authTime = Math.floor(Date.now() / 1000); // fresh, still a password
    return true;
  }).catch((caught) => caught);
  assert.ok(error instanceof CratestackHttpError);
  assert.deepEqual(error.stepUp?.unmet, ["amr"]);
  assert.equal(server.seen.length, 2);
});

test("the banner is keyed on the refusal code, not only the status", () => {
  const response = (status: number, body: unknown) =>
    new CratestackHttpError(new Response(null, { status }), body);
  const forbidden = response(403, { code: "FORBIDDEN", message: "no" });
  const attestation = response(401, {
    code: "UNAUTHORIZED",
    message: "expired",
    error: "attestation_expired",
  });
  assert.equal(refusalCode(forbidden), "FORBIDDEN");
  assert.equal(refusalCode(attestation), "attestation_expired");
  assert.equal(attestation.stepUp, undefined, "a 401 that is not a step-up");
  assert.equal(refusalCode(new Error("offline")), undefined);
  assert.equal(refusalText("FORBIDDEN"), "You are not allowed to do this.");
});
