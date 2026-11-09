// The COSE signed transport for the browser (issue #1010, ADR 0006): seal
// requests and open responses with `cratestack-cose`, compiled to WASM. The
// signature, the hash, the canonical query and the AAD are all Rust's; this
// file carries bytes in and out, and calls the external signer (if any) with
// the to-be-signed bytes the WASM hands it.
//
// A separate entry point on purpose: `@cratestack/cbor-web` (the codec) keeps
// its own WASM module, byte for byte as it was, and this one loads a second
// module, built from the same crate with its `cose` feature, only for a
// client that signs. See `docs/design/ts-cose-signed-mode.md`.
import {
  asCoseFailure,
  type CoseCallBinding,
  type CoseClient,
  type CoseClientOptions,
  CoseFailure,
  type CoseOpened,
  type CoseSignerOptions,
} from "@cratestack/ts-types";
import init, {
  canonicalQuery,
  contractHeaderValue,
  ClientEnvelope as WasmClientEnvelope,
} from "./wasm-pkg-cose/cratestack_cbor_wasm.js";

export type {
  CoseAlgName,
  CoseCallBinding,
  CoseClient,
  CoseClientOptions,
  CoseExternalSign,
  CoseOpened,
  CoseServerKey,
  CoseSignerOptions,
} from "@cratestack/ts-types";
export { CoseFailure } from "@cratestack/ts-types";

// Memoized like the codec's: a failed instantiation is retried by the next call.
let initPromise: Promise<void> | null = null;

function ensureInitialized(): Promise<void> {
  if (!initPromise) {
    initPromise = init()
      .then(() => undefined)
      .catch((error: unknown) => {
        initPromise = null;
        throw error;
      });
  }
  return initPromise;
}

function construct(options: CoseClientOptions): WasmClientEnvelope {
  const { signer, serverKeys, audience } = options;
  const keys = serverKeys.map((key) => ({ alg: key.alg, bytes: key.bytes }));
  const pinned =
    options.fixedIat === undefined && options.fixedCti === undefined
      ? undefined
      : { fixedIat: options.fixedIat, fixedCti: options.fixedCti };
  return build(signer, keys, audience, pinned);
}

function build(
  signer: CoseSignerOptions,
  keys: { alg: string; bytes: Uint8Array }[],
  audience: string,
  pinned: object | undefined,
): WasmClientEnvelope {
  switch (signer.type) {
    case "hmac":
      return WasmClientEnvelope.hmac(signer.alg, signer.secret, keys, audience, pinned);
    case "ed25519-seed":
      return WasmClientEnvelope.ed25519Seed(signer.seed, keys, audience, pinned);
    case "p256-scalar":
      return WasmClientEnvelope.p256Scalar(signer.scalar, keys, audience, pinned);
    case "external": {
      // The WASM calls `sign` with the to-be-signed bytes. A throw or a
      // rejection is reported as the signer's failure, without the page's own
      // error text.
      return WasmClientEnvelope.external(
        signer.alg,
        signer.publicKey,
        signer.sign,
        keys,
        audience,
        pinned,
      );
    }
    default:
      throw new CoseFailure("misuse", "unknown signer type");
  }
}

async function failing<T>(call: () => T | Promise<T>): Promise<T> {
  try {
    return await call();
  } catch (error) {
    throw asCoseFailure(error);
  }
}

/**
 * Builds a {@link CoseClient}: the one-time WASM instantiation, then a signer
 * and the static trust root. Throws a {@link CoseFailure} `misuse` for a bad
 * key or an empty audience.
 *
 * @example
 * ```ts
 * const cose = await createCoseClient({
 *   audience: "payments",
 *   serverKeys: [{ alg: "ed25519", bytes: serverPublicKey }],
 *   signer: { type: "external", alg: "esp256", publicKey, sign: (tbs) =>
 *     crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, privateKey, tbs).then((s) => new Uint8Array(s)) },
 * });
 * ```
 */
export async function createCoseClient(options: CoseClientOptions): Promise<CoseClient> {
  await ensureInitialized();
  const envelope = await failing(() => construct(options));
  return {
    mode: envelope.mode as CoseClient["mode"],
    mediaType: envelope.mediaType,
    kid: envelope.kid,
    canonicalQuery: (rawQuery: string): string => canonicalQuery(rawQuery),
    contractHeader: (contractSha: Uint8Array): string => {
      try {
        return contractHeaderValue(contractSha);
      } catch (error) {
        throw asCoseFailure(error);
      }
    },
    selectorHeaders: (binding: CoseCallBinding): Record<string, string> => {
      try {
        return envelope.selectorHeaders(binding) as Record<string, string>;
      } catch (error) {
        throw asCoseFailure(error);
      }
    },
    sealRequest: (payload, binding) =>
      failing(async () => (await envelope.sealRequest(payload, binding)) as Uint8Array),
    openResponse: (body, binding, sealedRequest, status, responsePayloadType) =>
      failing(
        async () =>
          (await envelope.openResponse(
            body,
            binding,
            sealedRequest,
            status,
            responsePayloadType,
          )) as CoseOpened,
      ),
  };
}
