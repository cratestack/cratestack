// Real-bundler verification for @cratestack/cbor (issue #288): proves the
// umbrella package's `"browser"` exports condition actually resolves to
// @cratestack/cbor-web's WASM build under a real bundler (Vite), the way
// cbor-smoke.ts already proved that for @cratestack/cbor-web directly
// (issue #287). This module is independent of both the notes app and
// cbor-smoke.ts: it runs once on page load, exercises the umbrella
// package's root entry point AND its explicit `/web` escape-hatch
// subpath, and reports pass/fail into its own status line. A build
// (`pnpm run build`) that resolves `@cratestack/cbor`'s `"browser"`
// condition to the same WASM asset `@cratestack/cbor-web` ships — not a
// second, duplicate `.wasm` — is the actual acceptance criterion; this
// module exercising both entry points at runtime is the browser-side
// proof.
import { createCborCodec as createCborCodecDefault } from '@cratestack/cbor';
import { createCborCodec as createCborCodecEscapeHatch } from '@cratestack/cbor/web';

const statusEl = document.getElementById('cbor-umbrella-status') as HTMLDivElement | null;

async function roundTrip(
  label: string,
  createCborCodec: () => Promise<{
    contentType: string;
    encode(value: unknown): unknown;
    decode(bytes: Uint8Array): unknown;
  }>,
): Promise<string> {
  const codec = await createCborCodec();
  const input = { hello: 'cratestack-umbrella', count: 3, note: null };

  const encoded = codec.encode(input) as Uint8Array;
  const decoded = codec.decode(encoded);

  const matches =
    typeof decoded === 'object' &&
    decoded !== null &&
    Object.keys(input).every(
      (key) =>
        JSON.stringify((decoded as Record<string, unknown>)[key]) ===
        JSON.stringify(input[key as keyof typeof input]),
    );

  if (!matches) {
    throw new Error(`${label} round-trip mismatch: ${JSON.stringify(decoded)}`);
  }
  return `${label}: ${codec.contentType}, ${encoded.byteLength} bytes`;
}

async function runCborUmbrellaSmokeCheck(): Promise<void> {
  if (!statusEl) return;
  try {
    const rootResult = await roundTrip('root (@cratestack/cbor, "browser" condition)', createCborCodecDefault);
    const escapeHatchResult = await roundTrip(
      'escape hatch (@cratestack/cbor/web)',
      createCborCodecEscapeHatch,
    );
    statusEl.textContent = `✓ ${rootResult} | ✓ ${escapeHatchResult}`;
  } catch (error) {
    statusEl.textContent = `✗ @cratestack/cbor failed: ${(error as Error).message}`;
  }
}

void runCborUmbrellaSmokeCheck();
