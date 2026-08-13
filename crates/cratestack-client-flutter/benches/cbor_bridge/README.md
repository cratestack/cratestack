# `cratestack_cbor` bridge vs `package:cbor` — benchmark

Compares `cratestack_client_flutter::cbor`'s `flutter_rust_bridge` entry points
(`encode_json`/`decode_json`, see `../../src/cbor/mod.rs`) against the pure-Dart
`package:cbor` the generated Dart clients use today, on a realistic payload.
Real, executed numbers are recorded below (cratestack#563) — not carried over
from the maintainer's original ~55x/~1000x estimate on a different stack.

## Why this isn't a `cargo bench` in this crate

The thing being measured is Dart-side FFI call overhead plus the Rust-side
codec, so it has to run as compiled Dart calling a compiled native library —
there is no way to measure it from inside `cargo test`/`cargo bench`. That is
also why this directory holds documentation and a ready-to-drop-in Dart
script rather than a runnable Rust target: reproducing it needs the same
frb-glue-generation step described in the crate's module docs and the root
`justfile`'s `frb-generate` recipe (cratestack#563 decision: glue is
generated on demand, never committed — see `src/cbor/mod.rs` and
`justfile`'s `--exclude embedded_flutter_native` comment for the full
rationale, which this crate now shares).

## Reproducing

This crate (`cratestack-client-flutter`) is a library — it deliberately does
not itself depend on `flutter_rust_bridge` or carry `mod frb_generated;` (see
`src/cbor/mod.rs`'s module docs), so running this benchmark means creating a
small native crate that wraps `cbor::encode_json`/`cbor::decode_json` with
`#[frb(sync)]`, exactly the pattern the published `cratestack_cbor` package's
own native crate will use (out of scope for this PR — see the ticket).

```bash
mkdir -p /tmp/cratestack-cbor-bench/src/api && cd /tmp/cratestack-cbor-bench

cat > Cargo.toml <<'EOF'
[package]
name = "cratestack_cbor_bench_native"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]
name = "cratestack_cbor_bench_native"

[dependencies]
flutter_rust_bridge = "=2.12.0"
# Point this at your checkout of cratestack.
cratestack-client-flutter = { path = "<repo>/crates/cratestack-client-flutter", default-features = false }
EOF

cat > src/lib.rs <<'EOF'
mod frb_generated;
pub mod api;
EOF

cat > src/api/mod.rs <<'EOF'
use flutter_rust_bridge::frb;

// #[frb(sync)] matters — see "Sync vs async matters a lot" below. The
// naive async-default binding measured SLOWER than pure-Dart package:cbor.
#[frb(sync)]
pub fn cbor_encode_json(json: String) -> Result<Vec<u8>, String> {
    cratestack_client_flutter::cbor::encode_json(json).map_err(|e| e.message)
}

#[frb(sync)]
pub fn cbor_decode_json(bytes: Vec<u8>) -> Result<String, String> {
    cratestack_client_flutter::cbor::decode_json(bytes).map_err(|e| e.message)
}
EOF

cat > flutter_rust_bridge.yaml <<'EOF'
rust_input: crate::api
rust_root: ./
dart_output: dart_out/
EOF

mkdir -p dart_out
cat > dart_out/pubspec.yaml <<'EOF'
name: cratestack_cbor_bench
environment:
  sdk: ^3.5.0
dependencies:
  flutter_rust_bridge: 2.12.0
  ffi: ^2.1.0
  cbor: ^6.5.1
EOF

(cd dart_out && dart pub get)
flutter_rust_bridge_codegen generate   # or: just frb-generate /tmp/cratestack-cbor-bench
cargo build --release

cp bench.dart dart_out/bench.dart      # this file, from this directory
(cd dart_out && dart run bench.dart)
```

`bench.dart` in this directory is the actual benchmark source — copy it in
as shown above (it is hand-written, not generated, so it's committed here
even though the surrounding native/Dart scaffold above is not).

## Sync vs async matters a lot

flutter_rust_bridge defaults to routing every call through an async
port/isolate dispatch, even for a synchronous computation like a codec call.
The first version of this benchmark used that default and measured the
bridge **slower** than pure-Dart `package:cbor` (0.5x — i.e. 2x slower) on a
small payload, purely from per-call async dispatch overhead dwarfing the
actual codec work. Annotating both entry points `#[frb(sync)]` (the same
attribute this crate's own README already shows for
`execute_streamed`/`rpc_call_streamed`) removes that overhead and is what
the numbers below use. **Any future native crate wrapping this module's
`encode_json`/`decode_json` must use `#[frb(sync)]`, or it will regress
below pure-Dart, not just underperform the original estimate.**

## Measured results (2026-08-13, this sandbox)

Toolchain: Dart 3.12.1, Flutter 3.44.1, flutter_rust_bridge_codegen 2.12.0,
`cargo build --release`, `x86_64-unknown-linux-gnu`. `package:cbor` 6.5.1.
Both `dart run` (JIT) and `dart compile exe` (AOT) measured; each cell is
encode+decode per iteration, single-threaded, after a warmup loop.

| Payload | Size (JSON / CBOR) | Mode | `package:cbor` | `cratestack_cbor` (frb, sync) | Speedup |
|---|---|---|---|---|---|
| Single model (11 scalar fields) | 541 B / 470 B | JIT (`dart run`) | 18.69 us/iter | 6.34 us/iter | **2.95x** |
| Single model (11 scalar fields) | 541 B / 470 B | AOT (`dart compile exe`) | 21.26 us/iter | 6.04 us/iter | **3.52x** |
| List page, 50 rows | 18,795 B / 16,569 B | JIT (`dart run`) | 690.70 us/iter | 193.19 us/iter | **3.58x** |
| List page, 50 rows | 18,795 B / 16,569 B | AOT (`dart compile exe`) | 846.49 us/iter | 192.52 us/iter | **4.40x** |

**Honest comparison against the ticket's motivation:** the maintainer's
original estimate (~55x minimum, ~1000x average, "on a separate stack") is
**not** reproduced here. The measured range is **~3–4.4x**, consistently,
across both payload sizes and both JIT and AOT Dart. This is real speedup —
worth having on a hot path that runs on every request — but it is a
different order of magnitude than the ticket's motivating number, and that
gap is reported here rather than omitted.

**Why the gap is plausible, not a bug in this measurement:** this bridge's
boundary type is JSON text (see `src/cbor/mod.rs`'s module docs for why —
flutter_rust_bridge has no dynamic "any JSON value" wire type the way napi
or wasm-bindgen do). Both sides of the comparison already pay Dart's
`jsonEncode`/`jsonDecode` cost building the input map; what differs is only
the CBOR encode/decode itself, plus one `serde_json::from_str`/`to_string`
pair on the Rust side of the bridge that a more direct value-marshaling
design (passing typed fields across FFI instead of a JSON string) would not
pay. A future, more direct bridge design could plausibly close some of this
gap — worth a follow-up measurement if that's ever built — but is out of
scope here (see cratestack#563's "generator seam" discussion).
