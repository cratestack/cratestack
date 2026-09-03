// Alpine (musl) smoke test for the freshly-built native addon — cratestack#850.
//
// Runs INSIDE `node:22-alpine`, against the package directory mounted at
// /pkg, and is the whole point of the musl legs: it proves the `.node` this
// job just built actually LOADS under musl and produces the right bytes,
// rather than proving only that `cargo` exited 0. Before this existed, the
// glibc-only build failed at exactly this point for every Alpine consumer
// while CI stayed green.
//
// It deliberately drives the package's own generated `native.mjs` loader
// (not `require` on the `.node` path directly): the loader is what does musl
// detection and platform dispatch, and dispatching to a name nothing ships
// was the actual defect.
//
// Not a substitute for the vitest suite — this asserts one known-good vector
// end to end, in the environment that was broken.
const PKG = process.env.PKG_DIR ?? "/pkg";

// Encoded with the framework's own Rust CborCodec. Canonical CBOR: map keys
// sort, so this is stable across platforms and is what makes byte-identical
// wire behavior an assertion rather than a claim.
const EXPECTED_HEX = "a365636f756e74016568656c6c6f65776f726c64666e6573746564830102f6";
const VALUE = { hello: "world", count: 1, nested: [1, 2, null] };

const { encode, decode } = await import(`${PKG}/native.mjs`);

const hex = Buffer.from(encode(VALUE)).toString("hex");
if (hex !== EXPECTED_HEX) {
  console.error(`FAIL encode: expected ${EXPECTED_HEX}`);
  console.error(`             got      ${hex}`);
  process.exit(1);
}

// Compared with keys sorted: the decoded object carries canonical CBOR key
// order, which is not the source literal's order.
const sortKeys = (v) =>
  JSON.stringify(v, (_k, x) =>
    x && typeof x === "object" && !Array.isArray(x)
      ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => a.localeCompare(b)))
      : x,
  );

const back = decode(Buffer.from(hex, "hex"));
if (sortKeys(back) !== sortKeys(VALUE)) {
  console.error(`FAIL decode: expected ${sortKeys(VALUE)}`);
  console.error(`             got      ${sortKeys(back)}`);
  process.exit(1);
}

console.log(`ok: loaded under ${process.platform}/${process.arch} (musl), encode+decode match`);
console.log(`    hex     : ${hex}`);
console.log(`    decoded : ${JSON.stringify(back)}`);
