// Fallback ambient type declarations for `@cratestack/cbor-node` and
// `@cratestack/cbor-web`.
//
// This package's `dependsOn` for its own `build` task (see turbo.json's
// `@cratestack/cbor#build` override) depends on @cratestack/ts-types'
// build but deliberately NOT on @cratestack/cbor-node's or
// @cratestack/cbor-web's — it does NOT force those two to be freshly
// compiled first, because their builds need Rust/wasm toolchains this
// repo's toolchain-free `js` CI job doesn't have. That means `tsc`
// compiling src/node.ts and src/web.ts sometimes runs against a
// workspace where those two siblings' own emitted `dist/*.d.ts`
// genuinely doesn't exist yet (unbuilt).
//
// TypeScript only consults an ambient `declare module` block for a given
// specifier when real file-based resolution can't find a match — when
// the siblings ARE built (local dev with the Rust/wasm toolchains, or
// this repo's dedicated `js-cbor-napi`/`js-cbor-wasm` CI jobs), their
// real, richer declarations resolve normally and these ambient
// declarations are simply unused. This file is a typecheck-time safety
// net for the toolchain-free lane, not the source of truth for either
// package's actual API — see their own `src/index.ts` for that.
declare module "@cratestack/cbor-node" {
  import type { CratestackRpcCodec } from "@cratestack/ts-types";

  export const cborCodec: CratestackRpcCodec;

  /** Which side of an op a body belongs to. */
  export type CompactDirection = "request" | "response";

  /**
   * The compact-wire-key tables of one client (`docs/design/compact-wire-keys.md`):
   * swaps schema field names for integer keys on the wire and back. Pure
   * functions of bytes; a bad input throws a catchable `Error`.
   */
  export interface CompactKeys {
    /** Named CBOR bytes to compact CBOR bytes. */
    toCompact(named: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
    /** Compact CBOR bytes back to named CBOR bytes. */
    fromCompact(compact: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
    /** Named JSON text to compact JSON text (keys become decimal strings). */
    toCompactJson(named: string, op: string, direction: CompactDirection): string;
    /** Compact JSON text back to named JSON text. */
    fromCompactJson(compact: string, op: string, direction: CompactDirection): string;
    /** The `Cratestack-Contract` header value of the op's current contract. */
    selector(op: string): string | undefined;
  }
  export function createCompactKeys(tables: Uint8Array): Promise<CompactKeys>;
}

declare module "@cratestack/cbor-web" {
  import type { CratestackRpcCodec } from "@cratestack/ts-types";

  export function createCborCodec(): Promise<CratestackRpcCodec>;

  /** Which side of an op a body belongs to. */
  export type CompactDirection = "request" | "response";

  /**
   * The compact-wire-key tables of one client (`docs/design/compact-wire-keys.md`):
   * swaps schema field names for integer keys on the wire and back. Pure
   * functions of bytes; a bad input throws a catchable `Error`.
   */
  export interface CompactKeys {
    /** Named CBOR bytes to compact CBOR bytes. */
    toCompact(named: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
    /** Compact CBOR bytes back to named CBOR bytes. */
    fromCompact(compact: Uint8Array, op: string, direction: CompactDirection): Uint8Array;
    /** Named JSON text to compact JSON text (keys become decimal strings). */
    toCompactJson(named: string, op: string, direction: CompactDirection): string;
    /** Compact JSON text back to named JSON text. */
    fromCompactJson(compact: string, op: string, direction: CompactDirection): string;
    /** The `Cratestack-Contract` header value of the op's current contract. */
    selector(op: string): string | undefined;
  }
  export function createCompactKeys(tables: Uint8Array): Promise<CompactKeys>;
}

// The `./cose` subpaths of the same two packages (cratestack#1010): the signed
// transport. Same role as above, a typecheck-time net for the toolchain-free
// lane; the shape is `@cratestack/ts-types`' `CoseModule`.
declare module "@cratestack/cbor-node/cose" {
  import type { CoseClient, CoseClientOptions } from "@cratestack/ts-types";

  export function createCoseClient(options: CoseClientOptions): Promise<CoseClient>;
  export { CoseFailure } from "@cratestack/ts-types";
}

declare module "@cratestack/cbor-web/cose" {
  import type { CoseClient, CoseClientOptions } from "@cratestack/ts-types";

  export function createCoseClient(options: CoseClientOptions): Promise<CoseClient>;
  export { CoseFailure } from "@cratestack/ts-types";
}
