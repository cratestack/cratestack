# Your first contribution

A start-to-finish walkthrough for someone who has never touched this repository.
It assumes you can use git and a terminal, and nothing else. It does **not**
assume you know Rust well, or anything about CrateStack's internals.

Budget: about 30 minutes, most of it the first `cargo build`.

## 1. What to work on

Good first targets, roughly easiest first:

- **A typo, a stale command, or a broken link.** Real contributions. Send them.
- Anything labelled [`good first issue`](https://github.com/cratestack/cratestack/labels/good%20first%20issue).
- Anything labelled [`help wanted`](https://github.com/cratestack/cratestack/labels/help%20wanted).
- A bug *you* hit. You already have the reproduction, which is the hard part.

Comment on the issue to say you're taking it, so two people don't do the same
work. You don't need permission to start — just say so.

If there's no issue yet, [file one first](./filing-an-issue.md). Our PR process
requires a linked source of truth, and an issue is the easiest one to link.

## 2. Set up

You need the Rust toolchain and [`just`](https://github.com/casey/just). The
toolchain version is pinned in `rust-toolchain.toml` (currently **1.98.0**) and
rustup will install it for you on first build — you don't need to pick a version.

```bash
git clone https://github.com/cratestack/cratestack.git
cd cratestack
cargo install just
```

**On Linux, install the GTK/WebKit dev packages before building the workspace.**
The `tauri-*` example crates pull `glib-sys`/`webkit2gtk-sys`, and without these
a workspace build fails on a fresh checkout:

```bash
sudo apt-get install libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
                     libjavascriptcoregtk-4.1-dev
```

macOS uses the system WebKit and needs nothing extra. Windows: see
[tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).

Now build:

```bash
cargo build --workspace --exclude embedded_flutter_native
```

`--exclude embedded_flutter_native` is not optional. That crate needs
`flutter_rust_bridge`-generated glue that is deliberately not committed, so a
bare `--workspace` build fails with `E0583`. Every recipe in this repository
excludes it for the same reason.

Expect the first build to take a while. Later builds are incremental.

## 3. Find your way around

You almost certainly don't need to understand the whole thing. Two facts get you
most of the way:

**Code generation happens at compile time, in `crates/cratestack-macros/`.** If
your change affects what a `.cstack` schema turns into, it's in there — organized
by concern (`model/`, `procedure/`, `view/`, `policy/`, `transport/`, `axum/`,
`client/`).

**Everything else is layered.** `cratestack-parser` reads schemas →
`cratestack-core` / `-policy` / `-sql` hold shared types → `cratestack-macros`
generates → the backend runtimes (`-sqlx`, `-rusqlite`, `-axum`) and the client
crates consume. Dependencies flow one way, and CI enforces it.

The fastest way to locate the code for a behaviour is to grep for a string you
saw in the output — an error message, a generated identifier, a CLI flag.

## 4. Make the change

Two conventions that will otherwise bite you in review:

**Keep files under ~200 lines.** This is why `macros/` and `axum/` are nested so
deeply. When a file would grow past the ceiling, split it by concern instead.
There's a CI job (`file-length ceiling`) that checks this.

**REST and RPC ship together.** If your change touches what goes over the wire —
query parameters, response shapes, client call surfaces — it must land on *both*
transports in the same pull request, including the generated Rust, Dart, and
TypeScript clients. This is cheap by design: RPC dispatch synthesizes a URL query
string and re-enters the REST parsing path
(`crates/cratestack-axum/src/rpc/synthesize.rs`), so the server side is usually
one struct field and one `pairs.push`. Shipping REST-only has cost us three
follow-up PRs before; the rule exists because of that.

Add a test. If you fixed a bug, the honest test is one that **fails before your
fix and passes after**. Confirm that by stashing your fix and watching it go red —
a test that passes both ways is testing the wrong thing.

## 5. Check your work

```bash
just all-checks
cargo test --workspace --exclude embedded_flutter_native
```

`just all-checks` is the canonical gate: formatting, `cargo fix`, clippy with
`-D warnings`, a full check, and `cargo deny`. Run *it*, not your own
reconstruction of it — the flags it sets are deliberate.

Two flags never to add:

- **Not `--all-features`.** It enables both mutually-exclusive `decimal-*`
  backends and trips a `compile_error!` in `cratestack-core`.
- **Not a bare `--workspace`** without the exclude, per above.

### About "all tests passed"

Postgres-backed tests (`banking_*`, `policy_db_*`, `generated_client_rust`) **skip
silently** when no database is configured — and a skipped test prints `ok`. A
green `cargo test` does not mean you exercised those paths.

If your change touches the server backend, run them for real:

```bash
just test-pg
```

That brings the Postgres container in `compose.yml` up and tears it down on exit,
even on failure.

**On rootless Docker**, testcontainers-based suites silently skip and still print
`ok`, because `testcontainers-rs` doesn't read `docker context`. `docker info`
succeeding proves nothing — that's the CLI, which does read it. Make a skip fail
loudly:

```bash
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
export CRATESTACK_REQUIRE_DB=1
just test-pg-tc
```

There's a fuller write-up in [CONTRIBUTING.md](../../CONTRIBUTING.md).

## 6. Add a changelog entry

If your change is user-visible, add a line under the `## Unreleased` heading at
the **top** of `CHANGELOG.md`. Don't create that heading yourself and don't file
under the newest dated section — that one belongs to a released version. A CI job
checks this.

## 7. Open the pull request

```bash
git checkout -b fix/short-description
git commit -m "fix(parser): reject duplicate @id on mixin fields"
git push -u origin fix/short-description
```

Then open the PR using the [template](../../.github/PULL_REQUEST_TEMPLATE.md),
which fills in automatically. Three sections are enforced by a CI check
(`AI Governance`) and the PR stays red until they're filled in:

1. **A source of truth** — the issue number or a URL. `Fixes #123` counts.
2. **Verification evidence** — the commands you ran and their output. Paste the
   real thing; "tests pass" without output is not evidence.
3. **An AI Usage Declaration** — tick what applies, including "Not used".

That third one is not a trap and it's not a judgement. This project's
[governance](https://adorsys-gis.github.io/ai-governance/) is explicit that AI may
accelerate the work but humans own intent, verification, and consequences. Using
an assistant is fine. Declaring it honestly is required. Submitting code you
can't explain in review is the actual line.

## 8. Review

A maintainer will review. Expect questions — they're about the change, not about
you. If something in the process was confusing, say so in the PR: that's a
documentation bug, and this page is where it gets fixed.

---

Stuck at any step? [Open a question issue](https://github.com/cratestack/cratestack/issues/new?template=question.yml).
Getting stuck here is a bug in this guide.
