# Filing an issue

This page exists because our issue list has two very different kinds of form on
it, and the difference isn't obvious from the names. Read the first section, pick
a form, and stop reading.

## Pick a form

| You want to… | Use | Effort |
| --- | --- | --- |
| Report something broken | [🐞 Bug report](https://github.com/cratestack/cratestack/issues/new?template=bug-report.yml) | 5 minutes |
| Ask how to do something | [💬 Question](https://github.com/cratestack/cratestack/issues/new?template=question.yml) | 2 minutes |
| Flag wrong/missing docs | [📖 Documentation problem](https://github.com/cratestack/cratestack/issues/new?template=docs.yml) | 2 minutes |
| Suggest a feature | [💡 Idea / feature request](https://github.com/cratestack/cratestack/issues/new?template=feature-request.yml) | 5 minutes |
| Plan work you intend to do here | [Epic](https://github.com/cratestack/cratestack/issues/new?template=epic.yml) · [User Story](https://github.com/cratestack/cratestack/issues/new?template=user-story.yml) · [Development Ticket](https://github.com/cratestack/cratestack/issues/new?template=dev-ticket.yml) | 20+ minutes |

**The first four are for you.** They ask only what a maintainer genuinely can't
work without.

**The last three are internal planning forms.** They demand an intent statement,
a linked source of truth, acceptance criteria, a test plan, verification
evidence, and a named accountable human, because that's what our
[AI governance discipline](https://adorsys-gis.github.io/ai-governance/) requires
of work we commit to doing. If a maintainer accepts your bug report or idea,
**they** write that ticket. You are never expected to.

Security vulnerabilities do not go in the issue tracker at all — see
[SECURITY.md](../../SECURITY.md).

## What makes a report we can act on

You do not need all of this. In rough order of how much it helps:

### 1. The smallest schema that still breaks

This is worth more than everything else combined. CrateStack generates code at
compile time from your `.cstack` file, so a schema we can paste into a file is a
reproduction we can run in under a minute.

To shrink one: delete a model, re-run, and see if it still fails. Keep deleting
until the failure goes away, then put the last thing back. What's left is your
repro — usually five to fifteen lines.

```cstack
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Post {
  id Int @id
  title String
}
```

### 2. The whole error, not the last line

Macro expansion errors put the useful part *above* the final message — the
generated span, the type it was trying to build, the field it choked on. Paste
the full output.

If the error is huge, `cargo build 2>&1 | head -100` is a good slice.

### 3. Exactly what you ran

```bash
cargo run -p cratestack-cli -- check --schema schema.cstack
cargo build -p my-service
```

Commands beat prose descriptions of commands.

### 4. Which surface

CrateStack has four facades that share a name (`cratestack` via Cargo's
`package =` rename), so "cratestack is broken" is genuinely ambiguous. The form
has a dropdown; "Not sure" is an acceptable answer and we'll work it out.

### 5. Versions

```bash
cratestack --version      # or: cargo run -p cratestack-cli -- --version
rustc --version
```

If you're on a git checkout rather than a release, `git rev-parse --short HEAD`.

## Things you don't need to do

- **You don't need to diagnose it.** "This fails and I don't know why" is a
  complete report. A wrong guess about the cause costs us less than a missing
  repro.
- **You don't need to check whether it's already fixed on `main`.** Nice if you
  do; not expected.
- **You don't need to propose a fix.** If you have one, open a pull request and
  link it — but the report stands on its own.
- **You don't need to fill in the governance fields.** Those live on the
  planning forms, not the reporting ones.

## After you file

A maintainer triages new issues and applies labels. What the labels mean:

| Label | Meaning |
| --- | --- |
| `needs-review` | Not yet triaged. Applied automatically by the beginner forms. |
| `good first issue` | Scoped small and self-contained. Claim it by commenting. |
| `help wanted` | We'd like outside help on this one. |
| `bug` / `enhancement` / `documentation` / `question` | Triage outcome. |
| `ticket` / `user-story` / `epic` | Converted into planned work. |

If your report becomes a Development Ticket, the ticket links back to your issue
as its source of truth. That's the governance rule working in your favour: your
report is the evidence the work is real.

Want to fix it yourself? → [Your first contribution](./first-contribution.md)
