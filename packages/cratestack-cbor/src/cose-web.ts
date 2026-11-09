// Browser entry of `@cratestack/cbor/cose` (resolved by the `"browser"` and `"default"` conditions):
// the COSE signed transport over WASM. See
// `@cratestack/cbor-web/cose` for the API; `@cratestack/ts-types` has its types.

export { createCoseClient } from "@cratestack/cbor-web/cose";
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
