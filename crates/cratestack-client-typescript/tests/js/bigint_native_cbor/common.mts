// Shared helpers for the smoke scripts `tests/native_cbor_bigint_encode.rs`
// runs against the generated package and the REAL, published
// `@cratestack/cbor`. Every `check` failure is a thrown Error, which makes the
// script exit non-zero and puts its message in the Rust test's report.
import { createCborCodec } from "@cratestack/cbor";

/** i64::MAX, i64::MIN and 2^53 + 1: the three values the cross-language
 *  fixtures pin (ADR 0019 D2). */
export const VALUES = ["9223372036854775807", "-9223372036854775808", "9007199254740993"] as const;

export function check(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

export function hex(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("hex");
}

export function includesBytes(haystack: Uint8Array, needle: Uint8Array): boolean {
  return Buffer.from(haystack).includes(Buffer.from(needle));
}

export async function realCodec() {
  return createCborCodec();
}

/** The CBOR a `BigInt` must be on the wire: a TEXT string (major type 3) of
 *  its canonical digits. `i64::MAX` is `73` plus 19 ASCII digits. */
export async function textStringBytes(wire: string): Promise<Uint8Array> {
  const codec = await realCodec();
  const bytes = new Uint8Array(codec.encode(wire) as Uint8Array);
  check(bytes[0]! >> 5 === 3, `${wire} should encode as CBOR major type 3, got ${hex(bytes)}`);
  check(bytes.length === wire.length + 1, `${wire} should be a one-byte header plus its digits`);
  return bytes;
}

export async function cborResponse(value: unknown): Promise<Response> {
  const codec = await realCodec();
  return new Response(new Uint8Array(codec.encode(value) as Uint8Array), {
    status: 200,
    headers: { "Content-Type": "application/cbor" },
  });
}

/** The bytes the real codec produces for `value`, as a plain `Uint8Array`. */
export async function encoded(value: unknown): Promise<Uint8Array> {
  const codec = await realCodec();
  return new Uint8Array(codec.encode(value) as Uint8Array);
}
