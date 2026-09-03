#!/usr/bin/env bash
# Tests for .github/scripts/npm-publish.sh's failure classification.
#
# Usage: npm-publish-tests.sh [path-to-wrapper]   (default: the tracked one)
#
# A fake `npm` is put first on PATH. Each scenario is a directory of
# canned outputs — `attempt1.out`/`attempt1.rc`, `attempt2.out`/... — that
# the fake replays in order, counting invocations, so a test can assert
# BOTH the exit code and how many times the wrapper actually retried.
# The canned outputs are the real lines npm printed on the v0.11.1
# release (run 33808493207), not paraphrases: the informational
# "Provenance statement published to transparency log" notice is present
# on every attempt, because that notice is precisely what the first
# version of the wrapper mis-matched.
#
# The decisive case is `permanent_error_is_not_retried`: run this file
# against the pre-fix wrapper (`git show <old>:.github/scripts/npm-publish.sh`)
# and that case fails — the old classifier retried a permanent E403 four
# times because the notice line contains the words "transparency log".
# Nothing here touches the network or a real registry.

set -uo pipefail

WRAPPER="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.github/scripts/npm-publish.sh}"
[ -x "$WRAPPER" ] || { echo "not executable: $WRAPPER" >&2; exit 2; }

SANDBOX=$(mktemp -d)
trap 'rm -rf "$SANDBOX"' EXIT
mkdir -p "$SANDBOX/bin" "$SANDBOX/pkg"
echo '{"name":"@cratestack/fake","version":"0.0.0"}' > "$SANDBOX/pkg/package.json"

# The fake npm: replays $FAKE_NPM_DIR/attempt<N>.{out,rc}; records argv.
cat > "$SANDBOX/bin/npm" <<'FAKE'
#!/usr/bin/env bash
n=$(cat "$FAKE_NPM_DIR/count" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$FAKE_NPM_DIR/count"
printf '%s\n' "$*" >> "$FAKE_NPM_DIR/argv.log"
f="$FAKE_NPM_DIR/attempt$n"
[ -f "$f.out" ] || f="$FAKE_NPM_DIR/attemptLAST"
cat "$f.out"; exit "$(cat "$f.rc")"
FAKE
chmod +x "$SANDBOX/bin/npm"
export PATH="$SANDBOX/bin:$PATH"
export NPM_PUBLISH_BACKOFF_SECONDS=0

NOTICE='npm notice Publishing to https://registry.npmjs.org/ with tag latest and public access
npm notice publish Signed provenance statement with source and build information from GitHub Actions
npm notice publish Provenance statement published to transparency log: https://search.sigstore.dev/?logIndex=2704425456'
E401='npm error code E401
npm error 401 Unauthorized - PUT https://registry.npmjs.org/@cratestack%2fcbor-node-darwin-arm64 - Failed to generate Web Auth URLs due to error: BadRequestError: token is invalid'
E409_STAGED='npm error code E409
npm error 409 Conflict - PUT https://registry.npmjs.org/@cratestack%2fcbor-node-darwin-arm64 - Cannot publish over previously staged version "0.11.1".'
E409_PUBLISHED='npm error code E403
npm error 403 Forbidden - PUT https://registry.npmjs.org/@cratestack%2fx - You cannot publish over the previously published versions: 0.11.1.'
E403_PERMANENT='npm error code E403
npm error 403 Forbidden - PUT https://registry.npmjs.org/@cratestack%2fx - You do not have permission to publish "@cratestack/x". Are you logged in as the correct user?'
TLOG='npm error code TLOG_CREATE_ENTRY_ERROR
npm error error creating transparency log entry: an equivalent entry already exists in the transparency log'
OK='npm notice Your package is being processed and may take a few minutes to become available.
+ @cratestack/fake@0.0.0'

pass=0; fail=0
scenario() { # name; then attempts are added with `attempt N "out" rc`
  FAKE_NPM_DIR="$SANDBOX/$1"; rm -rf "$FAKE_NPM_DIR"; mkdir -p "$FAKE_NPM_DIR"; export FAKE_NPM_DIR
}
attempt() { printf '%s\n%s\n' "$NOTICE" "$2" > "$FAKE_NPM_DIR/attempt$1.out"; echo "$3" > "$FAKE_NPM_DIR/attempt$1.rc"; }
attempt_last() { printf '%s\n%s\n' "$NOTICE" "$1" > "$FAKE_NPM_DIR/attemptLAST.out"; echo "$2" > "$FAKE_NPM_DIR/attemptLAST.rc"; }
check() { # name expected_rc expected_attempts [expected-stderr-substring]
  local name=$1 want_rc=$2 want_n=$3 want_msg=${4:-}
  local err; err=$("$WRAPPER" "$SANDBOX/pkg" --access public 2>&1 >/dev/null); local rc=$?
  local n; n=$(cat "$FAKE_NPM_DIR/count")
  if [ "$rc" -eq "$want_rc" ] && [ "$n" -eq "$want_n" ] && { [ -z "$want_msg" ] || printf '%s' "$err" | grep -q -- "$want_msg"; }; then
    echo "ok   $name (rc=$rc attempts=$n)"; pass=$((pass + 1))
  else
    echo "FAIL $name: rc=$rc (want $want_rc) attempts=$n (want $want_n)${want_msg:+ msg-match=$(printf '%s' "$err" | grep -q -- "$want_msg" && echo yes || echo NO)}"; fail=$((fail + 1))
  fi
}

scenario success_first_try;              attempt 1 "$OK" 0
check success_first_try 0 1

scenario web_auth_401_then_success;      attempt 1 "$E401" 1; attempt 2 "$OK" 0
check web_auth_401_then_success 0 2 "Failed to generate Web Auth URLs"

scenario tlog_conflict_then_success;     attempt 1 "$TLOG" 1; attempt 2 "$OK" 0
check tlog_conflict_then_success 0 2 "sigstore tlog conflict"

# THE DECISIVE CASE. The notice line is present (it always is); the error
# is a permanent 403. Exactly one attempt, non-zero exit.
scenario permanent_error_is_not_retried; attempt 1 "$E403_PERMANENT" 1
check permanent_error_is_not_retried 1 1 "non-transient"

scenario previously_staged_is_accepted;  attempt 1 "$E409_STAGED" 1
check previously_staged_is_accepted 0 1 "STAGED upstream"

scenario already_published_is_success;   attempt 1 "$E409_PUBLISHED" 1
check already_published_is_success 0 1 "already published"

scenario web_auth_401_gives_up_at_max;   attempt_last "$E401" 1
check web_auth_401_gives_up_at_max 1 4 "giving up"

# v0.11.1's darwin-arm64 sequence verbatim: three 401s, then the 409
# "previously staged". Old wrapper: failure after 4. New: accepted at 4.
scenario v0_11_1_darwin_arm64_sequence;  attempt 1 "$E401" 1; attempt 2 "$E401" 1; attempt 3 "$E401" 1; attempt 4 "$E409_STAGED" 1
check v0_11_1_darwin_arm64_sequence 0 4 "STAGED upstream"

# Rehearsal: one --dry-run invocation, nothing else.
scenario rehearsal_is_dry_run;           attempt 1 "$OK" 0
NPM_PUBLISH_REHEARSAL=1 "$WRAPPER" "$SANDBOX/pkg" --access public --provenance >/dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ] && [ "$(cat "$FAKE_NPM_DIR/count")" -eq 1 ] && grep -q -- '--dry-run' "$FAKE_NPM_DIR/argv.log" && ! grep -q -- '--provenance' "$FAKE_NPM_DIR/argv.log"; then
  echo "ok   rehearsal_is_dry_run"; pass=$((pass + 1))
else
  echo "FAIL rehearsal_is_dry_run: rc=$rc argv=$(cat "$FAKE_NPM_DIR/argv.log")"; fail=$((fail + 1))
fi

echo "npm-publish-tests: $pass passed, $fail failed (wrapper: $WRAPPER)"
[ "$fail" -eq 0 ]
