// The COSE signed transport for Node (issue #1010, ADR 0006): seal requests
// and open responses with `cratestack-cose` through the native addon built
// from `crates/cratestack-cbor-napi`. The signature, the hash, the canonical
// query and the AAD are all Rust's; this file carries bytes in and out, and
// calls the external signer (if any) with the to-be-signed bytes the addon
// hands it between the two steps of a seal.
import {
  asCoseFailure,
  type CoseCallBinding,
  type CoseClient,
  type CoseClientOptions,
  CoseFailure,
  type CoseModule,
  type CoseOpened,
  type CoseSignerOptions,
} from "@cratestack/ts-types";
import {
  type BindingInput,
  canonicalQuery,
  contractHeaderValue,
  ClientEnvelope as NativeClientEnvelope,
} from "../native.mjs";

export type {
  CoseAlgName,
  CoseCallBinding,
  CoseClient,
  CoseClientOptions,
  CoseExternalSign,
  CoseModule,
  CoseOpened,
  CoseServerKey,
  CoseSignerOptions,
} from "@cratestack/ts-types";
export { CoseFailure } from "@cratestack/ts-types";

/** A failure of the addon as a {@link CoseFailure}: the Rust errors carry
 *  `code` `rejected` or `misuse`; anything else napi raises (a binding of the
 *  wrong JS shape) is misuse too. */
function failure(thrown: unknown): unknown {
  const known = asCoseFailure(thrown);
  if (known instanceof CoseFailure) {
    return known;
  }
  if (thrown instanceof Error && typeof (thrown as { code?: unknown }).code === "string") {
    return new CoseFailure("misuse", thrown.message);
  }
  return known;
}

function guard<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw failure(error);
  }
}

// The binding as napi types it: mutable arrays, and absent members as
// `undefined`, which napi reads as `None`. A runtime check of its shape is the
// addon's (misuse), not this file's.
const native = (binding: CoseCallBinding): BindingInput => binding as unknown as BindingInput;

// `new Uint8Array(...)` re-wraps native return values for the same
// typed-array-generics reason as `cborCodec.encode`.
const bytes = (value: Uint8Array): Uint8Array => new Uint8Array(value);

function construct(options: CoseClientOptions): NativeClientEnvelope {
  const { signer, audience } = options;
  const serverKeys = options.serverKeys.map((key) => ({ alg: key.alg, bytes: key.bytes }));
  const pinned =
    options.fixedIat === undefined && options.fixedCti === undefined
      ? undefined
      : {
          ...(options.fixedIat === undefined ? {} : { fixedIat: options.fixedIat }),
          ...(options.fixedCti === undefined ? {} : { fixedCti: options.fixedCti }),
        };
  const signing: CoseSignerOptions = signer;
  switch (signing.type) {
    case "hmac":
      return NativeClientEnvelope.hmac(signing.alg, signing.secret, serverKeys, audience, pinned);
    case "ed25519-seed":
      return NativeClientEnvelope.ed25519Seed(signing.seed, serverKeys, audience, pinned);
    case "p256-scalar":
      return NativeClientEnvelope.p256Scalar(signing.scalar, serverKeys, audience, pinned);
    case "external":
      return NativeClientEnvelope.external(
        signing.alg,
        signing.publicKey,
        serverKeys,
        audience,
        pinned,
      );
    default:
      throw new CoseFailure("misuse", "unknown signer type");
  }
}

/** The caller's signature, or a `misuse` that does not echo the caller's own
 *  error text. */
async function callSigner(
  sign: (toBeSigned: Uint8Array) => Promise<Uint8Array>,
  toBeSigned: Uint8Array,
): Promise<Uint8Array> {
  let signature: unknown;
  try {
    signature = await sign(bytes(toBeSigned));
  } catch {
    throw new CoseFailure("misuse", "the signing callback failed");
  }
  if (!(signature instanceof Uint8Array)) {
    throw new CoseFailure("misuse", "the signing callback did not return a Uint8Array");
  }
  return signature;
}

/**
 * Builds a {@link CoseClient}. Throws a {@link CoseFailure} `misuse` for a bad
 * key or an empty audience. Async for call-site parity with
 * `@cratestack/cbor-web/cose`; the native addon needs no initialization.
 */
export async function createCoseClient(options: CoseClientOptions): Promise<CoseClient> {
  const envelope = guard(() => construct(options));
  const external = options.signer.type === "external" ? options.signer.sign : undefined;
  return {
    mode: envelope.mode as CoseClient["mode"],
    mediaType: envelope.mediaType,
    kid: bytes(envelope.kid),
    canonicalQuery: (rawQuery) => guard(() => canonicalQuery(rawQuery)),
    contractHeader: (contractSha) => guard(() => contractHeaderValue(contractSha)),
    selectorHeaders: (binding: CoseCallBinding) =>
      guard(() =>
        Object.fromEntries(
          envelope.selectorHeaders(native(binding)).map(({ name, value }) => [name, value]),
        ),
      ),
    async sealRequest(payload, binding) {
      try {
        if (external === undefined) {
          return bytes(envelope.sealRequest(payload, native(binding)));
        }
        // The seal runs up to the signer, which hands over what to sign.
        const pending = envelope.beginSealRequest(payload, native(binding));
        return bytes(pending.finish(await callSigner(external, pending.toBeSigned)));
      } catch (error) {
        throw failure(error);
      }
    },
    async openResponse(body, binding, sealedRequest, status, responsePayloadType) {
      const opened = guard(() =>
        envelope.openResponse(body, native(binding), sealedRequest, status, responsePayloadType),
      );
      return {
        payload: bytes(opened.payload),
        payloadType: opened.payloadType,
        kid: bytes(opened.kid),
        alg: opened.alg as CoseOpened["alg"],
        thumbprint: bytes(opened.thumbprint),
      };
    },
  };
}

/** The module shape `@cratestack/cbor/cose` re-exports. */
export const cose: CoseModule = { createCoseClient };
