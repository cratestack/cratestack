## 1. Summary

This PR changes:

- [Change 1]
- [Change 2]
- [Change 3]

It solves:

- [Problem / ticket / story link]

---

## 2. Intent

The intent of this PR is:

> [Explain why this change exists.]

---

## 3. Scope

### In Scope

- [Included change 1]
- [Included change 2]

### Out of Scope

- [Excluded change 1]
- [Excluded change 2]

---

## 4. Verification

I verified this change by:

- [ ] Running automated tests
- [ ] Running manual tests
- [ ] Checking logs
- [ ] Checking metrics
- [ ] Testing error cases
- [ ] Testing permissions/security behavior
- [ ] Testing rollback or failure behavior, if relevant

Commands run:

```bash
[command 1]
[command 2]
```

Results:

```text
[paste result or link]
```

---

## 5. Screenshots / Evidence

Add evidence here:

* Screenshot: [link]
* Logs: [link]
* Metrics: [link]
* Recording: [link]

---

## 6. Risk Assessment

Risk level:

* [ ] Low
* [ ] Medium
* [ ] High

Potential risks:

* [Risk 1]
* [Risk 2]

Mitigation:

* [Mitigation 1]
* [Mitigation 2]

---

## 7. AI Usage Declaration

AI was used for:

* [ ] Understanding existing code
* [ ] Generating code
* [ ] Refactoring
* [ ] Generating tests
* [ ] Drafting documentation
* [ ] Reviewing the diff
* [ ] Not used

Human verification:

* [ ] I understand every meaningful change in this PR
* [ ] I checked generated code manually
* [ ] I checked generated tests manually
* [ ] I removed unsupported AI assumptions
* [ ] I accept responsibility for this PR

---

## 8. Reviewer Focus

Please focus your review on:

* [ ] Correctness
* [ ] Architecture
* [ ] Security
* [ ] Performance
* [ ] Tests
* [ ] Maintainability
* [ ] Product intent
* [ ] Edge cases

---

## 9. Docs & Skills Parity

A user-facing change has two companions: **[cratestack-docs](https://github.com/cratestack/cratestack-docs)**
(for humans) and **[cratestack-skills](https://github.com/cratestack/cratestack-skills)**
(for coding agents, installed with `npx skills add cratestack/cratestack-skills`).
Both drift silently, and in opposite ways — docs go stale, skills teach agents
to write code against a surface that no longer exists.

Fill in both lines. A link, or `n/a — <reason>`; a bare `n/a` is rejected by CI.

- docs:
- skills:

<!--
CI (`just verify-parity-declaration`) requires these two lines whenever this
PR adds a "### " entry under "## Unreleased" in CHANGELOG.md. It checks the
DECLARATION only — it cannot see the other repositories, so a green check is
not evidence that either is up to date. That part is yours.
-->
