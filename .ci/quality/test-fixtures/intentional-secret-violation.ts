// TEMPORARY test fixture — not real application code.
//
// Intentionally triggers the `ts-hardcoded-secrets` Semgrep rule
// (.ci/rules/semgrep/typescript-safety.yml, ERROR severity) to verify that
// reviewdog's -reporter=github-pr-review actually posts a PR review comment
// (visible under Conversation), not just a Check Run annotation.
//
// This PR is expected to fail its `quality` check and show a review
// comment. That's success, not a bug. Meant to be closed without merging
// once confirmed.
const config = {
  apiKey: "sk-test-FAKE1234567890abcdefFAKE",
};

export default config;
