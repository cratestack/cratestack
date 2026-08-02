// Browser entry point (resolved by the `"browser"` and `"default"`
// conditions of this package's root `exports["."]`, and directly
// importable as the `@cratestack/cbor/web` escape hatch — see
// package.json and README.md).
//
// @cratestack/cbor-web's own `createCborCodec()` (see its src/index.ts) is
// already the uniform async-factory shape this umbrella package
// standardizes on for both platforms — see src/node.ts's doc comment for
// why. Nothing to adapt here: this file exists so the escape-hatch subpath
// (`@cratestack/cbor/web`) and the root `"browser"`/`"default"` conditions
// have their own dedicated compiled entry point, distinct from
// src/node.ts, rather than sharing one file whose behavior would need to
// branch at runtime.
export { createCborCodec } from "@cratestack/cbor-web";
