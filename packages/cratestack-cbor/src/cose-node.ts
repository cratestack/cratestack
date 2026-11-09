// Node entry of `@cratestack/cbor/cose` (resolved by the `"node"` condition):
// the COSE signed transport over the native addon. See
// `@cratestack/cbor-node/cose` for the API; `@cratestack/ts-types` has its types.

export { createCoseClient } from "@cratestack/cbor-node/cose";
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
export type { CoseClientSource, OpContracts } from "./sealed.js";
export { createSealedFetch, createSealedLink } from "./sealed.js";
