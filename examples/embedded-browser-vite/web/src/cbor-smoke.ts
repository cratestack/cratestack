// Real-bundler verification for @cratestack/cbor-web (issue #287): this
// example already proves a wasm-bindgen crate's .wasm asset loads through
// Vite's `--target web` handling (cratestack-rusqlite, via worker.ts). The
// same question needed answering for the cbor-web package specifically —
// its own crate, its own wasm-pack build, its own bundler resolution path
// — rather than assuming "it worked for one wasm crate so it'll work for
// another." This module is deliberately independent of the notes app
// above it: it runs once on page load, encodes/decodes a small payload
// through @cratestack/cbor-web, and reports pass/fail into its own status
// line. A build (`pnpm run build`) that produces this package's `.wasm`
// as a real asset in `dist/` — not just a type-check — is the actual
// acceptance criterion; this module exercising it at runtime is the
// browser-side half of that proof.
import { createCborCodec } from '@cratestack/cbor-web';

const cborStatusEl = document.getElementById('cbor-status') as HTMLDivElement | null;

async function runCborSmokeCheck(): Promise<void> {
  if (!cborStatusEl) return;
  try {
    const codec = await createCborCodec();
    const input = { hello: 'cratestack', count: 2, note: null };

    // Synchronous after the one await above — no per-call await, matching
    // the CratestackRpcCodec contract.
    const encoded = codec.encode(input) as Uint8Array;
    const decoded = codec.decode(encoded);

    // Structural comparison, not JSON.stringify equality — decoding goes
    // through serde_json::Value, whose map has no defined key order, so
    // stringify output order isn't guaranteed to match the input literal
    // even when every value round-tripped correctly.
    const matches =
      typeof decoded === 'object' &&
      decoded !== null &&
      Object.keys(input).every(
        (key) =>
          JSON.stringify((decoded as Record<string, unknown>)[key]) ===
          JSON.stringify(input[key as keyof typeof input]),
      );
    cborStatusEl.textContent = matches
      ? `✓ @cratestack/cbor-web (${codec.contentType}) round-tripped ${encoded.byteLength} bytes via Vite`
      : `✗ @cratestack/cbor-web round-trip mismatch: ${JSON.stringify(decoded)}`;
  } catch (error) {
    cborStatusEl.textContent = `✗ @cratestack/cbor-web failed: ${(error as Error).message}`;
  }
}

void runCborSmokeCheck();
