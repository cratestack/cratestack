// Shared helpers for the smoke scripts `tests/native_cbor_uuid.rs` runs
// against the generated package and the REAL, published `@cratestack/cbor`.
import { createCborCodec } from "@cratestack/cbor";

export const TEXT = "67e55044-10b1-426f-9247-bb680e5fe0c8";
export const TEXT2 = "01234567-89ab-cdef-0123-456789abcdef";

/** The 16 bytes a server writes for `text`: a CBOR byte string (major type 2). */
export function uuidBytes(text: string): Uint8Array {
  return Uint8Array.from(Buffer.from(text.replaceAll("-", ""), "hex"));
}

export function check(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

export function hex(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("hex");
}

export async function realCodec() {
  return createCborCodec();
}

/** A response whose `Uuid` positions hold `Uint8Array`s, as the server's do:
 *  the real codec writes a `Uint8Array` as a CBOR byte string. */
export async function cborResponse(value: unknown): Promise<Response> {
  const codec = await realCodec();
  const bytes = new Uint8Array(codec.encode(value) as Uint8Array);
  return new Response(bytes, { status: 200, headers: { "Content-Type": "application/cbor" } });
}

/** `Uuid` request positions must be CBOR TEXT strings (major type 3), which
 *  the server accepts alongside its own 16-byte form. */
export async function textStringBytes(text: string): Promise<Uint8Array> {
  const codec = await realCodec();
  const bytes = new Uint8Array(codec.encode(text) as Uint8Array);
  check(bytes[0]! >> 5 === 3, `${text} should encode as CBOR major type 3, got ${hex(bytes)}`);
  return bytes;
}

export function includesBytes(haystack: Uint8Array, needle: Uint8Array): boolean {
  return Buffer.from(haystack).includes(Buffer.from(needle));
}
