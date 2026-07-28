// TEMPORARY test fixture — not real application code.
//
// Intentionally triggers the `ts-hardcoded-secrets` Semgrep rule
// (.ci/rules/semgrep/typescript-safety.yml, ERROR severity) to verify that
// the quality pipeline's reviewdog reporting actually fails a PR check on a
// newly introduced error-level finding — the whole point of this pipeline.
//
// This PR is expected to fail its `quality` check. That's success, not a
// bug. This file and the PR that introduces it are meant to be closed
// without merging once the check's failure is confirmed.
const config = {
  apiKey: "sk-test-FAKE1234567890abcdefFAKE",
};

export default config;
