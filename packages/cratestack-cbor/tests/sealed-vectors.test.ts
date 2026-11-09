// The sealing plumbing against the real Rust client (the native addon) and
// the shared known-answer vectors (issue #1010): a link or fetch that wraps a
// call must put on the wire exactly the bytes `cratestack-cose` produces for
// that request, and must open the vectors' answers. A fake server stands in:
// it checks what was sent and answers with the vector's response.
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import keys from "../../../crates/cratestack-cose/tests/vectors/keys.json" with { type: "json" };
import unary from "../../../crates/cratestack-cose/tests/vectors/unary.json" with { type: "json" };

const cborNodePkgDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../node_modules/@cratestack/cbor-node",
);
const built =
  existsSync(path.join(cborNodePkgDir, "dist/cose.js")) &&
  existsSync(path.join(cborNodePkgDir, "native.mjs"));
if (!built) {
  console.warn("@cratestack/cbor-node isn't built — skipping the sealed-vectors suite.");
}

const hex = (text: string): Uint8Array =>
  Uint8Array.from(text.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16));
const toHex = (bytes: Uint8Array): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
const byName = (name: string) =>
  unary.cases.find((c) => c.name === name) as (typeof unary.cases)[number];
const COSE = 'application/cose; cose-type="cose-sign1"';

describe.skipIf(!built)("sealed calls against the shared vectors (native addon)", () => {
  async function client(audience = "payments") {
    const { createCoseClient } = await import("@cratestack/cbor/cose");
    const request = byName("rpc-request-sign1-ed25519-cti16");
    return createCoseClient({
      signer: { type: "ed25519-seed", seed: hex(keys.ed25519.seed) },
      serverKeys: [{ alg: "ed25519", bytes: hex(keys.ed25519.public) }],
      audience,
      fixedIat: request.iat as number,
      fixedCti: hex(request.cti as string),
    });
  }

  /** A fake server: asserts the sealed bytes, answers with `reply`. */
  const server = (expectBody: string, reply: Uint8Array) => {
    const seen: { headers: Headers; body: string }[] = [];
    const fetchFn = (async (_url: unknown, init?: RequestInit) => {
      seen.push({ headers: new Headers(init?.headers), body: toHex(init?.body as Uint8Array) });
      return new Response(reply as BodyInit, { headers: { "content-type": COSE } });
    }) as typeof fetch;
    return { seen, fetchFn, expectBody };
  };

  it("RPC: the link puts the vector's bytes on the wire and opens the vector's answer", async () => {
    const { createSealedLink } = await import("@cratestack/cbor/cose");
    const request = byName("rpc-request-sign1-ed25519-cti16");
    const answer = byName("rpc-response-sign1-ed25519");
    const fake = server(request.cose, hex(answer.cose));
    const link = createSealedLink({
      cose: await client(),
      contracts: { "model.Payment.create": request.binding.contract_sha },
    });
    const response = await link(
      { kind: "unary" as const, opId: "model.Payment.create", fetchFn: fake.fetchFn },
      (next) =>
        next.fetchFn("https://pay.example/rpc/model.Payment.create", {
          method: "POST",
          headers: { "content-type": "application/cbor", accept: "application/cbor" },
          body: hex(request.payload) as BodyInit,
        }),
    );
    expect(fake.seen[0]?.body).toBe(request.cose);
    expect(fake.seen[0]?.headers.get("content-type")).toBe(COSE);
    expect(fake.seen[0]?.headers.has("cratestack-payload-type")).toBe(false);
    expect(toHex(new Uint8Array(await response.arrayBuffer()))).toBe(answer.payload);
    expect(response.headers.get("content-type")).toBe("application/cbor");
  });

  it("REST: the fetch matches the template, binds query, Idempotency-Key and If-Match, and opens the answer", async () => {
    const { createSealedFetch, createCoseClient } = await import("@cratestack/cbor/cose");
    const request = byName("rest-request-sign1-ed25519-cti16");
    const answer = byName("rest-response-sign1-ed25519");
    const fake = server(request.cose, hex(answer.cose));
    const cose = await createCoseClient({
      signer: { type: "ed25519-seed", seed: hex(keys.ed25519.seed) },
      serverKeys: [{ alg: "ed25519", bytes: hex(keys.ed25519.public) }],
      audience: "payments",
      fixedIat: request.iat as number,
      fixedCti: hex(request.cti as string),
    });
    const sealedFetch = createSealedFetch({
      cose,
      fetch: fake.fetchFn,
      contracts: { "PUT /accounts/{account_id}/payments/{id}": request.binding.contract_sha },
    });
    const response = await sealedFetch(
      "https://pay.example/accounts/acc_42/payments/pay_7?dry_run=false",
      {
        method: "PUT",
        headers: { "idempotency-key": "idem-7f3a", "if-match": '"3"' },
        body: hex(request.payload) as BodyInit,
      },
    );
    expect(fake.seen[0]?.body).toBe(request.cose);
    expect(fake.seen[0]?.headers.get("idempotency-key")).toBe("idem-7f3a");
    expect(toHex(new Uint8Array(await response.arrayBuffer()))).toBe(answer.payload);
  });

  it("refuses a tampered answer, a wrong audience and an answer to another request", async () => {
    const { createSealedLink, CoseFailure } = await import("@cratestack/cbor/cose");
    const request = byName("rpc-request-sign1-ed25519-cti16");
    const answer = byName("rpc-response-sign1-ed25519");
    const flipped = hex(answer.cose);
    flipped[flipped.length >> 1] = (flipped[flipped.length >> 1] as number) ^ 1;
    for (const [why, audience, body, pin] of [
      ["a tampered answer", "payments", flipped, request],
      ["a wrong audience", "someone-else", hex(answer.cose), request],
      // The request actually sent differs (another payload), so the digest the
      // answer is bound to is not the digest of what was sent.
      ["an answer to another request", "payments", hex(answer.cose), { payload: "a0" }],
    ] as const) {
      const cose = await client(audience);
      const fake = server("", body);
      const link = createSealedLink({
        cose,
        contracts: { "model.Payment.create": request.binding.contract_sha },
      });
      const attempt = link(
        { kind: "unary" as const, opId: "model.Payment.create", fetchFn: fake.fetchFn },
        (next) =>
          next.fetchFn("https://pay.example/rpc/x", {
            method: "POST",
            body: hex(pin.payload) as BodyInit,
          }),
      );
      const error = await attempt.then(
        () => undefined,
        (thrown: unknown) => thrown,
      );
      expect(error, why).toBeInstanceOf(CoseFailure);
      expect(error, why).toMatchObject({ code: "rejected", message: "" });
    }
  });
});
