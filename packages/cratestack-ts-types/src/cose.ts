// The COSE signed transport's TypeScript surface (ADR 0006, issue #1010): the
// shape `@cratestack/cbor-web/cose` and `@cratestack/cbor-node/cose` both
// implement over `cratestack-cose`, and `@cratestack/cbor/cose` re-exports.
// Types only. All signing, hashing, the canonical query and the AAD live in
// Rust (wasm in the browser, napi in Node); TypeScript carries bytes to it.

/** The COSE algorithms, by the names the Rust side uses. `ed25519` and
 *  `esp256` (ECDSA P-256 over SHA-256, low-`s`) sign a COSE_Sign1;
 *  `hmac256-64` and `hmac256-256` MAC a COSE_Mac0. */
export type CoseAlgName = "ed25519" | "esp256" | "hmac256-64" | "hmac256-256";

/** A key the client trusts to answer: an Ed25519 public key (32 bytes), a
 *  SEC1 P-256 point (compressed or uncompressed) or an HMAC secret. */
export interface CoseServerKey {
  readonly alg: CoseAlgName;
  readonly bytes: Uint8Array;
}

/** Signs the complete to-be-signed bytes (the COSE `Sig_structure`) and
 *  resolves to the signature. Sign exactly the bytes given, never a digest of
 *  them: `crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, bytes)`
 *  for a P-256 key (WebCrypto's raw `r ‖ s`, or DER from a platform
 *  keystore, are both accepted), `crypto.subtle.sign("Ed25519", key, bytes)`
 *  for Ed25519. The key may be non-extractable. A signature that does not
 *  verify under the public key the envelope was built with is refused before
 *  anything is sent. */
export type CoseExternalSign = (toBeSigned: Uint8Array) => Promise<Uint8Array>;

/** Who signs. The key never leaves the `external` signer. */
export type CoseSignerOptions =
  /** A COSE_Mac0 envelope under a session key of at least 32 bytes. */
  | {
      readonly type: "hmac";
      readonly alg: "hmac256-64" | "hmac256-256";
      readonly secret: Uint8Array;
    }
  /** A COSE_Sign1 with an Ed25519 key held in memory (32-byte seed). */
  | { readonly type: "ed25519-seed"; readonly seed: Uint8Array }
  /** A COSE_Sign1 with a P-256 key held in memory (32-byte scalar). */
  | { readonly type: "p256-scalar"; readonly scalar: Uint8Array }
  /** A COSE_Sign1 whose signature `sign` makes (a non-extractable WebCrypto
   *  key, a platform keystore, a KMS). `publicKey` is the 32-byte Ed25519 key
   *  or the SEC1 P-256 point. */
  | {
      readonly type: "external";
      readonly alg: "ed25519" | "esp256";
      readonly publicKey: Uint8Array;
      readonly sign: CoseExternalSign;
    };

export interface CoseClientOptions {
  readonly signer: CoseSignerOptions;
  /** The keys a response may be signed with (the static trust root). */
  readonly serverKeys: readonly CoseServerKey[];
  /** The service the requests are sealed for; never empty. Bound in the AAD. */
  readonly audience: string;
  /** Tests only: pin the clock (seconds) and the request `cti`, to reproduce
   *  the shared known-answer vectors. */
  readonly fixedIat?: number;
  readonly fixedCti?: Uint8Array;
}

/** What a call is bound to (ADR 0006 §4). */
export interface CoseCallBinding {
  readonly method: string;
  /** The route template on REST (`/accounts/{id}`), the op id on RPC. */
  readonly route: string;
  readonly pathParams: readonly string[];
  /** The canonical query string, when the call has one. */
  readonly query?: string | undefined;
  /** The op's 32-byte contract digest (`OP_CONTRACTS`, decoded from hex). */
  readonly contractSha: Uint8Array;
  readonly idempotencyKey?: string | undefined;
  readonly ifMatch?: string | undefined;
  /** What the payload is encoded as; default `application/cbor`. Bound. */
  readonly payloadType?: string | undefined;
  /** The types the answer may come back in; default CBOR only. Bound. */
  readonly payloadAccept?: readonly string[] | undefined;
}

/** A response that verified. */
export interface CoseOpened {
  readonly payload: Uint8Array;
  /** The type the response was sealed under. */
  readonly payloadType: string;
  readonly kid: Uint8Array;
  readonly alg: CoseAlgName;
  readonly thumbprint: Uint8Array;
}

/** What a failed COSE call throws: `rejected` for every failed verification
 *  (no detail, by design: ADR 0006 §10, so the client is no more of an oracle
 *  than the server; the message is empty), `misuse` for a binding of the
 *  wrong shape, a bad key or a signer that failed (the message says which,
 *  and never carries the signer's own error text). */
export class CoseFailure extends Error {
  readonly code: "rejected" | "misuse";

  constructor(code: "rejected" | "misuse", message: string) {
    super(message);
    this.name = "CoseFailure";
    this.code = code;
  }
}

/** The `{ code, message }` the Rust side throws or rejects with, as a
 *  {@link CoseFailure}. Anything else (a wasm trap, an out-of-memory) passes
 *  through unchanged. */
export function asCoseFailure(thrown: unknown): unknown {
  if (thrown instanceof CoseFailure) {
    return thrown;
  }
  if (typeof thrown === "object" && thrown !== null) {
    const { code, message } = thrown as { code?: unknown; message?: unknown };
    if ((code === "rejected" || code === "misuse") && typeof message === "string") {
      return new CoseFailure(code, message);
    }
  }
  return thrown;
}

/** Seals requests and opens responses for one service. */
export interface CoseClient {
  /** `sign1` or `mac0`. */
  readonly mode: "sign1" | "mac0";
  /** The `Content-Type` (and `Accept`) of sealed bodies. */
  readonly mediaType: string;
  /** The signer's 8-byte `kid`. */
  readonly kid: Uint8Array;
  /** Seal `payload` (already encoded) as the request `binding` describes.
   *  Resolves to the body to send. Calls the external signer, if any, with
   *  the to-be-signed bytes. */
  sealRequest(payload: Uint8Array, binding: CoseCallBinding): Promise<Uint8Array>;
  /** The canonical form of a raw query string (the part after `?`), the form
   *  `binding.query` must carry. Computed by Rust, never re-implemented. */
  canonicalQuery(rawQuery: string): string;
  /** The `Cratestack-Contract` header value (11 characters) for the op's
   *  32-byte contract digest. The header is unbound; send it with every
   *  sealed call. */
  contractHeader(contractSha: Uint8Array): string;
  /** The payload-type selector headers the call needs sent with the body:
   *  `Cratestack-Payload-Type` when not CBOR, `Cratestack-Payload-Accept`
   *  when not just CBOR. `{}` for a default call. */
  selectorHeaders(binding: CoseCallBinding): Record<string, string>;
  /** Open the response `body` to `sealedRequest` (the exact bytes that were
   *  sent), which came back with HTTP `status`. `responsePayloadType` is the
   *  response's `Cratestack-Payload-Type` header (absent: CBOR). Rejects with
   *  a {@link CoseError} `rejected` for a tampered, replayed, wrong-audience,
   *  wrong-signer or unrequested-type answer. */
  openResponse(
    body: Uint8Array,
    binding: CoseCallBinding,
    sealedRequest: Uint8Array,
    status: number,
    responsePayloadType?: string | undefined,
  ): Promise<CoseOpened>;
}

/** The module `@cratestack/cbor/cose` (and the web and node packages'
 *  `/cose` subpaths) exports. */
export interface CoseModule {
  createCoseClient(options: CoseClientOptions): Promise<CoseClient>;
}
