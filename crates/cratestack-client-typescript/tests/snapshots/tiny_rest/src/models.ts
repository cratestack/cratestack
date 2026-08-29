import type { JsonValue } from "./runtime.js";
import DecimalJs from "decimal.js";

// cratestack#498: every `Decimal`-typed schema field is carried as a real
// arbitrary-precision value, not a wire-format-dependent opaque `string`.
// #495/#496 made `decimal-bigdecimal` a real server-side backend for
// values beyond `rust_decimal`'s ~28-29 significant-digit cap, but
// `rust_decimal`'s `Display` never emits scientific notation while
// `bigdecimal`'s does past a magnitude threshold (`"0.0000001"` vs.
// `"1E-7"` for the identical value) — a client that just typed the field
// `string` saw a backend-dependent format and had no way to do
// arithmetic/comparison on it at all. `decimal.js` parses both notations
// into the identical value (`new Decimal("1E-7").equals(new
// Decimal("0.0000001"))` is `true`), closing that gap.
//
// Cloned with an effectively-unbounded exponential-notation threshold
// (`toExpNeg`/`toExpPos`) rather than used directly: decimal.js's own
// default `.toString()`/`.toJSON()` switches to scientific notation past
// ±7/21 orders of magnitude, which would make even an *ordinary* value
// like `0.00000001` re-encode as `"1e-8"`. Every outbound encode path
// goes through one of these two: `JSON.stringify` (`jsonRpcCodec`, REST
// bodies) or `encodeWireFields`'s own `.toString()` call (the native
// `@cratestack/cbor` codec, see that function's doc comment below) — so
// this one unbounded threshold governs all of them uniformly. Forcing
// plain positional notation matches `rust_decimal`'s own `Display`
// exactly, so a re-encoded value is always accepted back by a server on
// *either* backend without needing to know whether it parses scientific
// notation.
export const Decimal = DecimalJs.clone({ toExpNeg: -1e9, toExpPos: 1e9 });
export type Decimal = DecimalJs;

/** One model/`type`'s own decode-time shape: `decimalKeys` /
 *  `bytesKeys` / `bytesListKeys` are its *direct* field wire names whose
 *  decoded values need converting out of their wire form, and `nested`
 *  maps a field name to the shape name (a {@link wireShapes} key) that
 *  field's own value should be revived against, for any field whose type
 *  is itself another declared model/`type`. See {@link wireShapes}'s doc
 *  comment for why this is keyed by structural path (via `nested`) rather
 *  than a single flat, schema-wide field-name set.
 *
 *  `Bytes` keys are split by arity because their wire forms are not
 *  self-identifying at every value: a populated `Bytes` is `number[]` and
 *  a populated `Bytes[]` is `number[][]`, but `[]` is both — an empty
 *  `Uint8Array` and an empty list of them are the same three characters
 *  on the wire. The schema knows which; the runtime cannot. */
export interface WireShape {
  readonly decimalKeys: readonly string[];
  readonly bytesKeys: readonly string[];
  readonly bytesListKeys: readonly string[];
  readonly nested: Readonly<Record<string, string>>;
}

/** One entry per model/`type` this schema declares (`crate::wire_shapes::
 *  build_wire_shapes`) — `reviveWireFields`'s registry.
 *
 *  cratestack#499 review: an earlier version of this revival scheme kept a
 *  single flat `Set<string>` of every `Decimal` field name reachable from
 *  a response's root type (its own fields *and* every relation's/`type`'s
 *  fields, unioned together) and matched it against a decoded response's
 *  keys at *any* nesting depth. That's provably unsound the moment two
 *  *different* reachable types can each contribute a field name to the
 *  same flat set: a non-`Decimal` field in one type that happens to share
 *  a name with a `Decimal` field in another reachable type gets wrongly
 *  converted. Confirmed empirically (not just theorized) with an
 *  `Order.total: Decimal` + related `Account.total: String` schema,
 *  `include`-ing the relation: a real (non-numeric) account reference
 *  threw `[DecimalError] Invalid argument]` decoding a perfectly valid
 *  response, and a numeric-looking one (`"00123"`) was silently corrupted
 *  into `Decimal("123")`, losing its leading zeros.
 *
 *  This registry fixes that by keeping every type's `Decimal` field names
 *  in *that type's own* `WireShape` only, never merged with another
 *  type's. `reviveShaped` (below) looks up a nested field's *own* shape
 *  via `nested` rather than testing the parent's key set against it, so
 *  `Account.total` is only ever checked against *Account's* shape (which,
 *  correctly, has no `total` key) — not `Order`'s. */
export const wireShapes: Readonly<Record<string, WireShape>> = {
  Widget: { decimalKeys: [], bytesKeys: [], bytesListKeys: [], nested: {  } },
};

/** Decodes `value` against the named entry in {@link wireShapes},
 *  replacing every string at a key that shape's own `decimalKeys` names
 *  with a real {@link Decimal} and every wire integer array at a
 *  `bytesKeys`/`bytesListKeys` name with a `Uint8Array`, and recursing
 *  into any key `nested` names using *that* field's own shape.
 *
 *  A `shapeName` not found in the registry (a plain scalar or enum
 *  return, e.g. `echoName(): string`) is a documented no-op fast path, so
 *  every model/procedure call site calls this unconditionally rather than
 *  the generator branching per call site. */
export function reviveWireFields(value: unknown, shapeName: string): unknown {
  const shape = wireShapes[shapeName];
  if (!shape) {
    return value;
  }
  return reviveShaped(value, shape);
}

/** {@link reviveWireFields} for a `Page<T>` envelope: applies `T`'s own
 *  shape to `.items` (a `Page` envelope's own keys — `items`/`totalCount`/
 *  `pageInfo` — are never themselves `T`'s fields, so `T`'s shape can't be
 *  applied to the envelope directly the way a plain `T`/`T[]` response
 *  can), leaving `totalCount`/`pageInfo` untouched. */
export function revivePagedWireFields(value: unknown, shapeName: string): unknown {
  const shape = wireShapes[shapeName];
  if (!shape || value === null || typeof value !== "object" || !("items" in value)) {
    return value;
  }
  const page = value as { items: unknown };
  return { ...page, items: reviveShaped(page.items, shape) };
}

function reviveShaped(value: unknown, shape: WireShape): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => reviveShaped(item, shape));
  }
  if (value !== null && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      const nestedShapeName = shape.nested[key];
      if (nestedShapeName !== undefined) {
        const nestedShape = wireShapes[nestedShapeName];
        result[key] = nestedShape ? reviveShaped(entry, nestedShape) : entry;
      } else if (shape.decimalKeys.includes(key) && typeof entry === "string") {
        result[key] = new Decimal(entry);
      } else if (shape.bytesKeys.includes(key)) {
        result[key] = toBytes(entry);
      } else if (shape.bytesListKeys.includes(key)) {
        result[key] = Array.isArray(entry) ? entry.map(toBytes) : entry;
      } else {
        result[key] = entry;
      }
    }
    return result;
  }
  return value;
}

/** One `Bytes` leaf, wire form -> `Uint8Array`.
 *
 *  `null` passes through untouched: a nullable `Bytes` column is `null`
 *  on the wire, and turning that into an empty `Uint8Array` would erase
 *  the difference between "no value" and "zero bytes" — the same
 *  distinction the generated `Uint8Array | null` field type keeps.
 *
 *  Anything that isn't an array is also passed through rather than
 *  coerced. The shape registry says this key *should* be `Bytes`, but a
 *  response that disagrees (a hand-rolled server, a proxy rewriting
 *  bodies, a schema/deployment skew) is better surfaced to the caller as
 *  the value it actually was than silently turned into empty bytes. */
function toBytes(value: unknown): unknown {
  return Array.isArray(value) ? Uint8Array.from(value as number[]) : value;
}

/** Counterpart to {@link reviveWireFields} for a procedure whose return
 *  type is a bare revivable scalar — `Decimal` or `Bytes`, at any arity —
 *  rather than an object with such fields. {@link reviveWireFields} only
 *  walks object/array *containers* looking for shaped properties, so a
 *  raw top-level decoded value needs this simpler counterpart instead
 *  (cratestack#498 F2: a procedure like `quote(): Decimal` was previously
 *  declared `Decimal` but decoded as an untouched `string`).
 *
 *  `kind` comes from `crate::wire_shapes::ScalarRevival`, rendered into
 *  the call site by the generator, because the wire form alone cannot say
 *  which conversion is wanted: `Bytes` and `Bytes[]` are `number[]` and
 *  `number[][]` when populated but both `[]` when empty. An unrecognised
 *  `kind` is a no-op rather than a throw — a generated client should never
 *  produce one, and degrading to the raw decoded value beats failing a
 *  response that is otherwise perfectly readable.
 *
 *  See `procedure_views.rs::ProcedureView::revival_kind` for which
 *  generated call sites use this vs. {@link reviveWireFields}. */
export function reviveWireScalar(value: unknown, kind: string): unknown {
  if (kind === "bytes") {
    // A bare `Bytes`/`Bytes?` return: one wire integer array (or `null`).
    return toBytes(value);
  }
  if (kind === "bytesList") {
    // A bare `Bytes[]` return: an array *of* wire integer arrays. Handled
    // here rather than by recursing through the `"bytes"` branch, which
    // would see the outer array and convert it whole.
    return Array.isArray(value) ? value.map(toBytes) : value;
  }
  if (kind !== "decimal") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => reviveWireScalar(item, kind));
  }
  return typeof value === "string" ? new Decimal(value) : value;
}

/** Encode-side counterpart to {@link reviveWireFields}/
 *  {@link reviveWireScalar} (cratestack#746 follow-up): every RPC
 *  request body — `create`/`update` inputs, procedure arguments (plain,
 *  nested, or array-valued), a `FindMany<Model>` argument's
 *  `<Model>Where`/`DecimalFilter` operators, and each frame's own `input`
 *  inside a `batch()` payload — can carry a real {@link Decimal} instance
 *  wherever the schema declares a `Decimal`-typed field, and every one of
 *  those needs to reach the wire as a plain string before it reaches a
 *  codec's `encode()`.
 *
 *  `jsonRpcCodec`'s `JSON.stringify` already handles this for free via
 *  `Decimal.prototype.toJSON` (an alias for `.toString()`, see the
 *  `Decimal` export's own doc comment above) — but `@cratestack/cbor`
 *  (the default codec since #746) walks a value's *own enumerable
 *  properties* on its way to a `serde_json::Value` and never calls
 *  `toJSON`. `decimal.js`'s `clone()` (used to build this file's
 *  `Decimal` export) assigns `constructor` as an own enumerable property
 *  pointing at a function, and a function has no `serde_json::Value`
 *  representation — so encoding any request body containing a real
 *  `Decimal` instance under the native codec throws `JS functions cannot
 *  be represented as a serde_json::Value` before a request is ever sent.
 *  Confirmed against the actually-published `@cratestack/cbor`, not
 *  theorized.
 *
 *  Deliberately NOT built on {@link wireShapes}/{@link reviveShaped}'s
 *  by-field-name registry, unlike the decode-side functions above: this
 *  direction already has a real, unambiguous {@link Decimal} instance in
 *  hand at every leaf, so `value instanceof Decimal` identifies exactly
 *  what needs converting with no risk of the field-name collision hazard
 *  {@link wireShapes}'s own doc comment (`crate::decimal`, cratestack#499)
 *  exists to avoid — and it needs no per-shape registry entry for
 *  `<Model>Where`/procedure-argument wrapper interfaces, neither of which
 *  {@link wireShapes} describes at all (it is built only for
 *  models/`type`s, which is all the *decode* side ever needs — see that
 *  module's doc comment). A plain recursive walk that only special-cases
 *  the one JS runtime type that can't survive the native codec covers
 *  every shape above uniformly.
 *
 *  Called unconditionally from `rpc-runtime.ts.j2`'s `terminalLink`/
 *  `rpc-stream-terminal.ts.j2`'s `terminalStreamLink` — the two places
 *  every unary/batch/stream request body reaches a codec's `encode()` —
 *  rather than gated on `native_cbor`: converting a `Decimal` to its
 *  `.toString()` form before `JSON.stringify` produces the byte-identical
 *  request body `jsonRpcCodec` already sends today (the same
 *  `.toString()` `Decimal.prototype.toJSON` calls), so applying it
 *  unconditionally is a true no-op for the JSON path and avoids a fourth
 *  `{% if native_cbor %}` branch in these already-branchy
 *  templates. */
/** Rewrites every `Uint8Array` in `value` into the plain integer array a
 *  `Bytes` field travels as on the wire, leaving everything else alone.
 *
 *  **JSON encode paths only** — {@link jsonRpcCodec} and the REST
 *  runtime's request body. The native `@cratestack/cbor` codec must keep
 *  receiving the real `Uint8Array`, because it encodes one as a CBOR byte
 *  string (RFC 8949 major type 2, cratestack#783), which is both correct
 *  and about half the bytes of the integer array. Applying this
 *  unconditionally would throw that away.
 *
 *  `JSON.stringify` cannot do this itself: a `Uint8Array` has no `toJSON`,
 *  so it serializes as an index-keyed *object* (`{"0":1,"1":2}`) that no
 *  server-side `Vec<u8>` can decode — the same defect cratestack#783 fixed
 *  on the CBOR side, in a different disguise.
 *
 *  A `JSON.stringify` replacer would be cheaper than this pre-walk but is
 *  not sufficient: Node's `Buffer` is a `Uint8Array` subclass (so it is
 *  assignable to a generated `Bytes` field) *and* defines its own
 *  `toJSON`, which `JSON.stringify` applies **before** the replacer ever
 *  sees the value — yielding `{"type":"Buffer","data":[...]}`. Walking the
 *  value first sidesteps `toJSON` entirely, so a `Buffer` and a plain
 *  `Uint8Array` encode identically. Measured, not assumed. */
export function encodeBinaryAsJson(value: unknown): unknown {
  if (value instanceof Uint8Array) {
    return Array.from(value);
  }
  if (Array.isArray(value)) {
    return value.map((item) => encodeBinaryAsJson(item));
  }
  // `instanceof Uint8Array` above already caught every typed array this
  // client's own types can produce; anything else object-shaped is walked
  // for nested binary. `Decimal` instances are deliberately NOT
  // special-cased here — `encodeWireFields` has already converted them by
  // the time a codec runs, and on the REST path `Decimal.prototype.toJSON`
  // handles them inside `JSON.stringify`.
  if (value !== null && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      result[key] = encodeBinaryAsJson(entry);
    }
    return result;
  }
  return value;
}

export function encodeWireFields(value: unknown): unknown {
  if (value instanceof Decimal) {
    return value.toString();
  }
  // Pass a `Bytes` value through UNTOUCHED (cratestack#820). Load-bearing
  // and easy to delete by accident, because it looks like a no-op.
  //
  // A `Uint8Array` is not `Array.isArray`, so without this it falls into
  // the generic object branch below and is rebuilt through
  // `Object.entries` into `{"0":1,"1":2,"2":3}`. That happens before
  // EITHER codec runs — `terminalLink` calls this function
  // unconditionally — so it broke both of them at once: the native codec
  // emitted a CBOR map where a byte string belongs, and on the JSON path
  // `encodeBinaryAsJson` never saw a real `Uint8Array` to convert, so its
  // `Array.from` never fired either. Measured on a real generated client,
  // not theorized.
  //
  // Returning it as-is is correct for both paths rather than a
  // native-only carve-out: the native codec wants the real typed array
  // (RFC 8949 major type 2, cratestack#783), and the JSON path wants
  // `encodeBinaryAsJson` to receive one so it can produce the integer
  // array — which is exactly what this restores.
  if (value instanceof Uint8Array) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => encodeWireFields(item));
  }
  if (value !== null && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      result[key] = encodeWireFields(entry);
    }
    return result;
  }
  return value;
}

// Mirrors cratestack-core::page::{Page, PageInfo} exactly — this is
// the literal wire shape every `@@paged` list route serializes with
// `#[serde(rename_all = "camelCase")]`, not an independently designed
// client-side type. Keep field names and optionality in lockstep with
// that struct; do not add/rename fields here without changing it
// there first.
export interface PageInfo {
  limit: number | null;
  offset: number | null;
  hasNextPage: boolean;
  hasPreviousPage: boolean;
}

export interface Page<T> {
  items: T[];
  totalCount: number | null;
  pageInfo: PageInfo;
}

// Mirrors cratestack-core::page::PageInput exactly — the request-side
// counterpart to Page/PageInfo above, currently usable only as a
// procedure argument type. Keep field names and optionality in lockstep
// with that struct.
export interface PageInput {
  limit: number | null;
  offset: number | null;
}

// Shared building blocks for every `<Model>Where`/`<Model>FindMany`
// pair below (search-with-filters for procedures — mirrors
// cratestack-core::find_many::FieldFilterInput and
// cratestack-macros's per-model `<Model>Where`/`<Model>SortField`/
// `<Model>OrderByClause`/`<Model>FindManyInput` exactly). Usable only
// as a procedure argument type.
export interface EqualityFilter<V> {
  eq?: V;
  ne?: V;
  in?: V[];
  isNull?: boolean;
}

export interface ComparableFilter<V> extends EqualityFilter<V> {
  lt?: V;
  lte?: V;
  gt?: V;
  gte?: V;
}

export interface StringFilter extends ComparableFilter<string> {
  contains?: string;
  startsWith?: string;
}

export type NumberFilter = ComparableFilter<number>;
export type BooleanFilter = EqualityFilter<boolean>;
export type UuidFilter = ComparableFilter<string>;
export type DateTimeFilter = ComparableFilter<string>;
// cratestack#498: `Decimal`, not `string` — see this file's own
// `Decimal`/`reviveWireFields` doc comments above for why. Unlike a
// response's own `Decimal` fields, a `DecimalFilter` only ever travels
// *outbound* as part of a `<Model>Where`/`FindMany` procedure argument
// (`find_many_views.rs`'s own doc comment — "usable only as a procedure
// argument type"), so it needs no `reviveWireFields` counterpart on
// decode. It DOES need `encodeWireFields` on encode, same as every
// other `Decimal`-carrying request body (see that function's own doc
// comment for why `Decimal.prototype.toJSON`/`JSON.stringify` alone isn't
// enough once `@cratestack/cbor` is the codec, cratestack#746 follow-up)
// — `rpc-runtime.ts.j2`'s `terminalLink` applies it uniformly to every
// outbound RPC request body, so a `FindMany<Model>` argument's `where`
// filter needs no separate glue at this call site either.
export type DecimalFilter = ComparableFilter<Decimal>;

export type SortDirection = "asc" | "desc";

export type WidgetSortField = 'id' | 'name' | 'weight';
export const WidgetSortFieldValues = [
  "id",
  "name",
  "weight",
] as const satisfies readonly WidgetSortField[];

export interface Widget {
  id?: number;
  name?: string;
  weight?: number | null;
}

export interface CreateWidgetInput {
  id: number;
  name: string;
  weight?: number | null;
}

export interface UpdateWidgetInput {
  name?: string;
  weight?: number | null;
}

export interface WidgetWhere {
  id?: NumberFilter;
  name?: StringFilter;
  weight?: NumberFilter;
}

export interface WidgetOrderByClause {
  field: WidgetSortField;
  direction: SortDirection;
}

export interface WidgetFindMany {
  where?: WidgetWhere;
  orderBy?: WidgetOrderByClause[];
}

export interface EchoNameArgs {
  name: string;
}

