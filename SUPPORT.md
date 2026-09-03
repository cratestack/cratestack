# Getting help

CrateStack is pre-1.0 and maintained by a small team. Here's where to go.

## I'm trying to learn how to use it

- **[Quickstart](https://cratestack.dev/getting-started/quickstart)** — the
  shortest path from nothing to a running schema.
- **[Documentation](https://cratestack.dev)** — guides and reference.
- **[Runnable examples](https://github.com/cratestack/cratestack/tree/main/examples)** —
  every deployment shape as a real project you can `cargo run`: Postgres server,
  embedded SQLite, browser wasm with OPFS, Flutter, React, Next.js, Tauri.
- **API docs** — [docs.rs/cratestack-pg](https://docs.rs/cratestack-pg) ·
  [docs.rs/cratestack-sqlite](https://docs.rs/cratestack-sqlite) ·
  [docs.rs/cratestack-client](https://docs.rs/cratestack-client) ·
  [docs.rs/cratestack-api](https://docs.rs/cratestack-api)

## I have a question the docs didn't answer

[Open a question issue](https://github.com/cratestack/cratestack/issues/new?template=question.yml).
It takes two minutes and there's no wrong way to ask. If the answer turns out to
be "the docs don't cover that", we relabel it as a documentation bug — that's on
us, not on you.

## Something is broken

[Open a bug report](https://github.com/cratestack/cratestack/issues/new?template=bug-report.yml).
The single most useful thing you can include is the smallest `.cstack` schema
that still reproduces it — see [filing an issue](docs/contributing/filing-an-issue.md).

## I found a security vulnerability

Do **not** open a public issue. Follow the disclosure process in
[SECURITY.md](SECURITY.md).

## I want to contribute

Start with [Your first contribution](docs/contributing/first-contribution.md),
then [CONTRIBUTING.md](CONTRIBUTING.md) for the full development workflow.

## Response times

This is not a commercial support channel. Issues are triaged as maintainer time
allows. A well-scoped report with a reproduction gets answered fastest, because
it's the one that can be acted on without a round trip.
