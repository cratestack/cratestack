// The conformance suite every `CoseModule` runs (issue #1010): the shared
// known-answer vectors of `crates/cratestack-cose/tests/vectors`, which the
// Rust native tests, the wasm tests and the napi tests already seal and open
// byte for byte. A package's test passes its module, the parsed vector files
// and its test runner's `describe`/`it`/`expect`; the suite itself imports
// no test framework and no signing code of its own: the only crypto here is
// WebCrypto standing in for a non-extractable key.
import { CoseFailure, type CoseModule } from "./cose.js";

/** The slice of a test runner the suite needs. */
export interface TestApi {
  describe(name: string, body: () => void): void;
  it(name: string, body: () => void | Promise<void>): void;
  // biome-ignore lint/suspicious/noExplicitAny: vitest's `expect` is generic over its subject.
  expect: (value: any, message?: string) => any;
}

interface KeyEntry {
  alg: number;
  kid: string;
  public?: string;
  public_sec1_uncompressed?: string;
  secret?: string;
  seed?: string;
  scalar?: string;
}
interface VectorBinding {
  method: string;
  route: string;
  path_params: string[];
  query: string | null;
  contract_sha: string;
  bound_headers: { idempotency_key: string | null; if_match: string | null };
  status: number | null;
}
interface VectorCase {
  name: string;
  direction: "request" | "response";
  key: string;
  iat: number | null;
  cti: string | null;
  payload: string;
  cose: string;
  binding: VectorBinding;
  request_digest_of: string | null;
}
export interface CoseVectors {
  keys: Record<string, KeyEntry>;
  unary: { cases: VectorCase[] };
}

export const hex = (text: string): Uint8Array =>
  Uint8Array.from(text.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16));
const toHex = (bytes: Uint8Array): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

const ALGS: Record<number, "ed25519" | "esp256" | "hmac256-64" | "hmac256-256"> = {
  [-19]: "ed25519",
  [-9]: "esp256",
  4: "hmac256-64",
  5: "hmac256-256",
};
const AUDIENCE = "payments";

const bindingOf = (b: VectorBinding) => ({
  method: b.method,
  route: b.route,
  pathParams: b.path_params,
  query: b.query ?? undefined,
  contractSha: hex(b.contract_sha),
  idempotencyKey: b.bound_headers.idempotency_key ?? undefined,
  ifMatch: b.bound_headers.if_match ?? undefined,
});

const trusted = (keys: Record<string, KeyEntry>, name: string) => {
  const entry = keys[name] as KeyEntry;
  const material = entry.public ?? entry.public_sec1_uncompressed ?? entry.secret ?? "";
  return [{ alg: ALGS[entry.alg] as "ed25519", bytes: hex(material) }];
};

const base64url = (bytes: Uint8Array): string =>
  btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");

/** A non-extractable WebCrypto key for a vector key, and its public half. */
async function webCryptoKey(entry: KeyEntry, p256: boolean): Promise<CryptoKey> {
  if (!p256) {
    const pkcs8 = new Uint8Array([
      ...hex("302e020100300506032b657004220420"),
      ...hex(entry.seed ?? ""),
    ]);
    return crypto.subtle.importKey("pkcs8", pkcs8, "Ed25519", false, ["sign"]);
  }
  const point = hex(entry.public_sec1_uncompressed ?? "");
  const jwk = {
    kty: "EC",
    crv: "P-256",
    d: base64url(hex(entry.scalar ?? "")),
    x: base64url(point.slice(1, 33)),
    y: base64url(point.slice(33)),
  };
  return crypto.subtle.importKey("jwk", jwk, { name: "ECDSA", namedCurve: "P-256" }, false, [
    "sign",
  ]);
}

const subtleSign = (key: CryptoKey, p256: boolean) => async (tbs: Uint8Array) =>
  new Uint8Array(
    await crypto.subtle.sign(
      p256 ? { name: "ECDSA", hash: "SHA-256" } : "Ed25519",
      key,
      tbs as BufferSource,
    ),
  );

/** ASN.1 DER of a raw `r ‖ s`, what a platform keystore returns. */
function der(raw: Uint8Array): Uint8Array {
  const integer = (bytes: Uint8Array): number[] => {
    let start = 0;
    while (start < bytes.length - 1 && bytes[start] === 0) start++;
    const digits = Array.from(bytes.slice(start));
    const pad = (digits[0] as number) & 0x80 ? [0] : [];
    return [0x02, digits.length + pad.length, ...pad, ...digits];
  };
  const body = [...integer(raw.slice(0, 32)), ...integer(raw.slice(32))];
  return Uint8Array.from([0x30, body.length, ...body]);
}

export function defineCoseConformance(
  cose: CoseModule,
  vectors: CoseVectors,
  { describe, it, expect }: TestApi,
): void {
  const { keys, unary } = vectors;
  const requests = unary.cases.filter((c) => c.direction === "request");
  const responses = unary.cases.filter((c) => c.direction === "response");
  const client = (key: string, pin?: VectorCase, audience = AUDIENCE) => {
    const entry = keys[key] as KeyEntry;
    const alg = ALGS[entry.alg] as CoseClientAlg;
    const signer =
      alg === "ed25519"
        ? { type: "ed25519-seed" as const, seed: hex(entry.seed ?? "") }
        : alg === "esp256"
          ? { type: "p256-scalar" as const, scalar: hex(entry.scalar ?? "") }
          : { type: "hmac" as const, alg, secret: hex(entry.secret ?? "") };
    return cose.createCoseClient({
      signer,
      serverKeys: trusted(keys, key),
      audience,
      ...(pin ? { fixedIat: pin.iat as number, fixedCti: hex(pin.cti as string) } : {}),
    });
  };
  const rejected = async (attempt: Promise<unknown>, why: string) => {
    const error = await attempt.then(
      () => undefined,
      (thrown: unknown) => thrown,
    );
    expect(error, `${why} must be refused`).toBeInstanceOf(CoseFailure);
    expect(error).toMatchObject({ code: "rejected", message: "" });
  };
  const request = (name: string) => unary.cases.find((c) => c.name === name) as VectorCase;

  describe("COSE vectors", () => {
    it("seals every request vector byte for byte", async () => {
      expect(requests).toHaveLength(17);
      for (const c of requests) {
        const sealed = await (await client(c.key, c)).sealRequest(
          hex(c.payload),
          bindingOf(c.binding),
        );
        expect(toHex(sealed), c.name).toBe(c.cose);
      }
    });

    it("an external WebCrypto key signs: Ed25519 vectors are byte for byte", async () => {
      const entry = keys.ed25519 as KeyEntry;
      const sign = subtleSign(await webCryptoKey(entry, false), false);
      let sealedCount = 0;
      for (const c of requests.filter((r) => r.key === "ed25519")) {
        let asked = 0;
        const external = await cose.createCoseClient({
          signer: {
            type: "external",
            alg: "ed25519",
            publicKey: hex(entry.public ?? ""),
            sign: (tbs) => {
              asked++;
              return sign(tbs);
            },
          },
          serverKeys: trusted(keys, "ed25519"),
          audience: AUDIENCE,
          fixedIat: c.iat as number,
          fixedCti: hex(c.cti as string),
        });
        const sealed = await external.sealRequest(hex(c.payload), bindingOf(c.binding));
        expect(toHex(sealed), c.name).toBe(c.cose);
        expect(asked).toBe(1);
        sealedCount++;
      }
      expect(sealedCount).toBe(5);
    });

    it("an external non-extractable P-256 key signs, raw or DER, always low-s", async () => {
      const entry = keys.p256 as KeyEntry;
      const key = await webCryptoKey(entry, true);
      expect(key.extractable).toBe(false);
      const raw = subtleSign(key, true);
      const half = BigInt("0x7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8");
      for (const sign of [raw, async (tbs: Uint8Array) => der(await raw(tbs))]) {
        const external = await cose.createCoseClient({
          signer: {
            type: "external",
            alg: "esp256",
            publicKey: hex(entry.public_sec1_uncompressed ?? ""),
            sign,
          },
          serverKeys: trusted(keys, "p256"),
          audience: AUDIENCE,
        });
        const c = requests.find((r) => r.key === "p256") as VectorCase;
        // WebCrypto's ECDSA is randomised, so about half the signatures come
        // out high-s: every one must be normalised before it is sent.
        for (let i = 0; i < 24; i++) {
          const sealed = await external.sealRequest(hex(c.payload), bindingOf(c.binding));
          const s = BigInt(`0x${toHex(sealed.slice(-32))}`);
          expect(s <= half).toBe(true);
          expect(sealed.length).toBe(hex(c.cose).length);
        }
      }
    });

    it("opens the signed responses, and refuses tampered ones with no detail", async () => {
      let opened = 0;
      for (const c of responses.filter((r) => r.request_digest_of)) {
        const sealedRequest = hex(request(c.request_digest_of as string).cose);
        const status = c.binding.status as number;
        const body = hex(c.cose);
        const mine = await client(c.key);
        const result = await mine.openResponse(body, bindingOf(c.binding), sealedRequest, status);
        expect(toHex(result.payload), c.name).toBe(c.payload);
        expect(toHex(result.kid)).toBe((keys[c.key] as KeyEntry).kid);
        expect(result.thumbprint).toHaveLength(32);
        expect(result.payloadType).toBe("application/cbor");
        opened++;
        const flipped = body.slice();
        flipped[Math.floor(body.length / 2)] = (flipped[Math.floor(body.length / 2)] as number) ^ 1;
        await rejected(
          mine.openResponse(flipped, bindingOf(c.binding), sealedRequest, status),
          "a flipped bit",
        );
        await rejected(
          mine.openResponse(body, bindingOf(c.binding), sealedRequest, status ^ 1),
          "another status",
        );
      }
      expect(opened).toBe(12);
    });

    it("refuses a wrong audience, a replayed answer, another signer and an unrequested type", async () => {
      const c = responses.find((r) => r.key === "ed25519" && r.request_digest_of) as VectorCase;
      const sealedRequest = hex(request(c.request_digest_of as string).cose);
      const body = hex(c.cose);
      const binding = bindingOf(c.binding);
      const status = c.binding.status as number;
      const mine = await client("ed25519");
      await mine.openResponse(body, binding, sealedRequest, status);
      await rejected(
        (await client("ed25519", undefined, "someone-else")).openResponse(
          body,
          binding,
          sealedRequest,
          status,
        ),
        "a wrong audience",
      );
      const other = requests.find((r) => r.key === "ed25519" && r.name !== c.request_digest_of);
      await rejected(
        mine.openResponse(body, binding, hex((other as VectorCase).cose), status),
        "an answer to another request",
      );
      const stranger = await cose.createCoseClient({
        signer: { type: "ed25519-seed", seed: hex((keys.ed25519 as KeyEntry).seed ?? "") },
        serverKeys: trusted(keys, "ed25519_other"),
        audience: AUDIENCE,
      });
      await rejected(
        stranger.openResponse(body, binding, sealedRequest, status),
        "an untrusted signer",
      );
      await rejected(
        mine.openResponse(
          body,
          binding,
          sealedRequest,
          status,
          "application/vnd.cratestack.compact+cbor",
        ),
        "an unrequested answer type",
      );
    });

    it("refuses a signer that fails or lies, without echoing its error text", async () => {
      const entry = keys.ed25519 as KeyEntry;
      const c = requests.find((r) => r.key === "ed25519") as VectorCase;
      const wrongKey = subtleSign(
        await webCryptoKey({ ...entry, seed: "99".repeat(32) }, false),
        false,
      );
      const signers: Record<string, (tbs: Uint8Array) => Promise<Uint8Array>> = {
        throws: () => {
          throw new Error("secret detail");
        },
        rejects: () => Promise.reject(new Error("secret detail")),
        "another key": wrongKey,
        "not bytes": (async () => "nope") as unknown as () => Promise<Uint8Array>,
        "too short": async () => new Uint8Array(63),
      };
      for (const [name, sign] of Object.entries(signers)) {
        const external = await cose.createCoseClient({
          signer: { type: "external", alg: "ed25519", publicKey: hex(entry.public ?? ""), sign },
          serverKeys: trusted(keys, "ed25519"),
          audience: AUDIENCE,
        });
        const error = await external.sealRequest(hex(c.payload), bindingOf(c.binding)).then(
          () => undefined,
          (thrown: unknown) => thrown,
        );
        expect(error, name).toBeInstanceOf(CoseFailure);
        expect((error as CoseFailure).code, name).toBe("misuse");
        expect((error as CoseFailure).message, name).not.toContain("secret detail");
      }
    });

    it("names misuse, describes itself, and computes the selector headers and query in Rust", async () => {
      const mine = await client("ed25519");
      expect([mine.mode, mine.kid.length]).toEqual(["sign1", 8]);
      expect(mine.mediaType).toContain("cose-sign1");
      expect((await client("hmac-256-64")).mode).toBe("mac0");
      const c = requests[0] as VectorCase;
      const bad = (binding: object) =>
        mine.sealRequest(hex(c.payload), binding as ReturnType<typeof bindingOf>);
      for (const binding of [
        { ...bindingOf(c.binding), contractSha: new Uint8Array(31) },
        { ...bindingOf(c.binding), payloadType: "application/cose" },
        { ...bindingOf(c.binding), pathParams: "nope" },
      ]) {
        await expect(bad(binding)).rejects.toMatchObject({ code: "misuse" });
      }
      const compact = "application/vnd.cratestack.compact+cbor";
      expect(mine.selectorHeaders(bindingOf(c.binding))).toEqual({});
      expect(
        mine.selectorHeaders({
          ...bindingOf(c.binding),
          payloadType: compact,
          payloadAccept: [compact, "application/cbor"],
        }),
      ).toEqual({
        "Cratestack-Payload-Type": compact,
        "Cratestack-Payload-Accept": `${compact}, application/cbor`,
      });
      expect(mine.contractHeader(hex(c.binding.contract_sha))).toHaveLength(11);
      expect(mine.canonicalQuery("b=2&a=1&a=0")).toBe("a=1&a=0&b=2");
      await expect(
        cose.createCoseClient({
          signer: { type: "ed25519-seed", seed: new Uint8Array(31) },
          serverKeys: [],
          audience: AUDIENCE,
        }),
      ).rejects.toMatchObject({ code: "misuse" });
    });
  });
}

type CoseClientAlg = "ed25519" | "esp256" | "hmac256-64" | "hmac256-256";
