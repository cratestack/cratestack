#!/usr/bin/env bash
# Build the device crate to wasm and generate the JS glue the page loads
# into web/pkg/ (gitignored). Needs the wasm32 target, a wasm-capable clang
# (see ../embedded-browser-vite/README.md), and a wasm-bindgen CLI whose
# version matches the `wasm-bindgen` crate in the workspace Cargo.lock.
set -euo pipefail
cd "$(dirname "$0")"
root=../..

want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }' || true)
if [[ "$have" != "$want" ]]; then
  echo "wasm-bindgen CLI $want required (found: ${have:-none}):" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

export CC_wasm32_unknown_unknown="${CRATESTACK_CC:-${CC:-clang}}"
cargo build -p sync-notes-client --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  "$root/target/wasm32-unknown-unknown/release/sync_notes_client.wasm"
echo "web/pkg ready — now: DATABASE_URL=postgres://… cargo run -p sync-notes-server"
