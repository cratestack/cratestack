#!/usr/bin/env bash
# Asserts the built `@cratestack/cbor-web` wasm package is codec-only
# (cratestack#1026): no COSE `ClientEnvelope` in the JS glue or in the module's
# export names. The class lives behind `cratestack-cbor-wasm`'s `cose`
# feature, which only the Dart package's web artifact is built with
# (`just cbor-vendor-web`); a feature on by default, or a `--features cose`
# copied into the npm build, would publish a signing client nobody
# documented. Run by ci.yml's `js-cbor-wasm` job on every PR and by
# release-cli.yml's `publish-npm-cbor-web` before the publish.
#
# Usage: .ci/cbor-web-codec-only.sh [dist/wasm-pkg directory]
set -euo pipefail

dist="${1:-packages/cratestack-cbor-web/dist/wasm-pkg}"
test -s "$dist/cratestack_cbor_wasm.js" || { echo "::error::$dist/cratestack_cbor_wasm.js is missing or empty — build @cratestack/cbor-web first" >&2; exit 1; }
test -s "$dist/cratestack_cbor_wasm_bg.wasm" || { echo "::error::$dist/cratestack_cbor_wasm_bg.wasm is missing or empty" >&2; exit 1; }
if grep -aEq 'sealRequest|openResponse|ClientEnvelope|clientenvelope_' "$dist/cratestack_cbor_wasm.js" "$dist/cratestack_cbor_wasm_bg.wasm"; then
  echo "::error::@cratestack/cbor-web's wasm exports the COSE ClientEnvelope — it must stay codec-only. Is the cratestack-cbor-wasm 'cose' feature on in this build? — DO NOT PUBLISH." >&2
  exit 1
fi
echo "ok: $dist is codec-only"
