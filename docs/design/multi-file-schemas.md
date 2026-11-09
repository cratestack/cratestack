# Multi-file schemas: `part` / `part of` to split, `import` to share

Status: **accepted** (2026-11-08), decided under delegation by the maintainer
(@stephane-segning), who delegated every decision of epic
[cratestack#910](https://github.com/cratestack/cratestack/issues/910) on 2026-11-08, including
cratestack#915 question 5, which the ticket reserved for the maintainer. Each answer below
states the rejected alternative and the reason. Implementation is tracked by the epic's children
(§17 lists which tickets this design closes).

> **Update (0.19.1):** implemented. The loader, the three macros' `SCHEMA_FILES`, the CLI (#914),
> the LSP (#921) and the split `react-vite-swr` example (#924) shipped in **0.19.0**;
> `examples/shared-types-import` and the LSP's rename refusal in a file another schema imports
> (§12) followed in **0.19.1** (`CHANGELOG.md`).

Date: 2026-11-08.

Scope: the `.cstack` grammar (`part "…"`, `part of "…"`, `import "…"`), the parser's file loader,
`SchemaError`, the three `include_*_schema!` macros (`SCHEMA_SOURCE`, a new `SCHEMA_FILES`,
`SCHEMA_SHA256`, rebuild tracking), every `cratestack` CLI command that takes `--schema`,
`cratestack-lsp`, Studio's schema load, and the one example that is split
(`examples/react-vite-swr`). The ADR is
[ADR 0023](https://github.com/cratestack/cratestack-docs/blob/main/internals/multi-file-schemas-adr.md)
in `cratestack-docs/internals/` (grammar decisions are user-visible, so the ADR lives there per
`docs/adr/README.md`); this document is its context.

**Evidence convention.** `path:line` anchors were READ on `main` at `a6f37c2` (v0.18.1).

## 1. What exists today

*As of v0.18.1, before the implementation; the rows on loading and rebuild tracking describe the
single-file loader that 0.19.0 replaced.*

| Fact | Where |
|---|---|
| Each macro reads one file with `std::fs::read_to_string`, which rustc does not track | `crates/cratestack-macros/src/include/parse.rs:17-28` |
| `pub const SCHEMA_SOURCE: &str = include_str!(…)` is the only thing that makes the schema a rebuild dependency | `include/server.rs:269`, `include/embedded.rs:231`, `include/client.rs:137` |
| `SCHEMA_SHA256` is **not** a hash of the text: it is `schema_digest`, the canonical whole-IR identity, which drops spans and `///` docs and sorts top-level declarations by name | `crates/cratestack-core/src/schema_identity.rs:1-26`, `include/schema_sha.rs` |
| Every span is a byte offset computed from `Line::start`, which `collect_lines` counts from 0 | `crates/cratestack-parser/src/line_helpers.rs:13-31` |
| Validation never reads the source text; it works on `Schema` alone | `crates/cratestack-parser/src/validate/mod.rs:234` (`let _ = (path, source);`) |
| `SchemaError` already carries `file` and `source_text` (cratestack#916, closed) | `crates/cratestack-parser/src/diagnostics.rs` |
| Mixins are expanded into models inside the parse, per file | `crates/cratestack-parser/src/parse/mod.rs:274` |
| No code generator reads a span | `grep` of `cratestack-macros`, `cratestack-client-*`, `cratestack-mock-wiremock`, `cratestack-migrate`: zero hits for `SourceSpan`/`name_span`/`.span.start` |
| `part` and `import` are reserved identifiers (cratestack#922, 0.13.0); the editor grammar highlights `part`, `part of`, `import` (#929); LSP completion offers both (#952) | `validate/reserved_idents.rs:24`, `packages/cratestack-vscode/syntaxes/cstack.tmLanguage.json:55-59`, `crates/cratestack-lsp/src/completion.rs:76-86` |
| A census test parses every committed `.cstack` | `crates/cratestack-parser/tests/committed_schemas.rs` |

Two of these decide most of the design. Because no generator reads a span, a merged schema
whose declarations equal a single file's produces the same generated code. Because the digest
ignores spans, docs and order, `SCHEMA_SHA256` of a split schema already equals that of its
single-file equivalent, with no new hashing scheme.

## 2. The grammar

Three new top-level directives. Each takes one line, a double-quoted path and nothing after it.
A trailing comment is not allowed, which matches `transport`.

```cstack
part "models/post.cstack"        // in a root: this file is more of me
part of "../schema.cstack"       // in a part: I belong to that root
import "../shared/money.cstack"  // anywhere: bring in a shared file's types
```

- `part of "` is matched before `part "`, because both start with `part `.
- `part`, `part of` or `import` with no quoted path, an unterminated quote, or text after the
  closing quote is a parse error: ``expected a quoted path after `part`, like `part "models/post.cstack"` ``.
- Directives may appear anywhere at top level. Placing them first is the documented style. A
  `///` doc comment above a directive is discarded, as it is for `transport`.
- `part` and `import` stay reserved identifiers. `part of` needs no separate reservation, because
  the reserved word `part` already makes any `part of …` identifier position an error (#922).
- The directives are **not** in the IR. They are returned beside the `Schema` in a
  `FileDirectives` value (§14) and never appear on `Schema`. That keeps `Schema`, its `Debug`
  output (`print-ir`), its JSON and `schema_identity`'s exhaustive destructuring byte-identical
  for every existing schema. This supersedes cratestack#990, which proposed two fields on
  `Schema`: those fields would add `parts: []` and `part_of: None` to every schema's `print-ir`
  output and to its serialized IR, which the epic rules out.

A worked example. This is the split `examples/react-vite-swr` ships (§16):

```cstack
// examples/react-vite-swr/schema.cstack: the root
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Caller {
  id Int
}

part "schema/board.cstack"
part "schema/task.cstack"
part "schema/procedures.cstack"
```

```cstack
// examples/react-vite-swr/schema/task.cstack
part of "../schema.cstack"

model Task {
  id Int @id
  title String
  done Boolean
  boardId Int
  board Board @relation(fields:[boardId],references:[id])

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}
```

`Board` is declared in `schema/board.cstack` and resolves here: a part shares one flat namespace
with its root and the other parts.

An import, for a shape two services both use:

```cstack
// shared/money.cstack: no `part of`, so it belongs to nobody
enum Currency {
  EUR
  XAF
}

type Money {
  amount Decimal
  currency Currency
}

mixin Timestamps {
  createdAt DateTime @default(now())
  updatedAt DateTime @default(now())
}
```

```cstack
// billing/schema.cstack
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

import "../shared/money.cstack"

model Invoice {
  @use(Timestamps)

  id Int @id
  total Decimal
  currency Currency
}

type QuoteResult {
  price Money
}
```

## 3. Why two mechanisms

`import` with a flat merge could split a schema on its own. `part` exists because of
**locking**: a part belongs to exactly one root and is not a schema by itself, while a shared file
may be imported by many schemas and is one. The `part of` back-reference is what lets `import`
refuse a fragment (§9, M10) and what lets the CLI and the LSP find the root of a file opened on
its own (§11, §12). The root's `part` list is what makes discovery explicit, which is what makes
rebuild tracking correct (§10). Dart has both halves for the same reasons.

The epic's own collapse test, "if that distinction stops being worth two grammars, collapse to
one", is answered by §5's Q5: imports carry only types, enums and mixins, parts carry anything a
root may, so the two now differ in what they may contain as well as in ownership.

## 4. Where each declaration may appear

| Declaration | Root | Part | Imported file, or a part of one |
|---|---|---|---|
| `datasource`, `auth`, `transport`, `mcp { }` | yes | no: M5 | no: M12 |
| `model`, `view`, `procedure`, `mutation procedure`, `subscription procedure`, `topic`, `query` | yes | yes | no: M12 |
| `extension <name> { }` | yes | yes (merged by union, as a repeat in one file is today) | no: M12 |
| `type`, `enum`, `mixin` | yes | yes | yes |
| `part "…"` | yes | no: M6 | in an imported root only |
| `part of "…"` | no, a root has none (M4 if one is given as a root) | exactly once (M7) | in a part of an imported root |
| `import "…"` | yes | yes | yes |

`datasource`, `auth`, `transport` and `mcp` are **root-only singletons**: one per schema, schema
wide, so a part that declared one would make "which one wins" depend on file order.

`extension` is not a singleton (it is a set, and a repeated block is already harmless), so
parts may declare one. Imports may not: an extension unlocks syntax tied to a Cargo feature of
the consuming crate, and a file someone else maintains must not switch a capability on in your
build. A shared type that needs `Vector(n)` therefore needs the importer to declare
`extension pgvector { }`. The merged schema reports the existing "requires extension" error at
the type, in the imported file.

## 5. `import` semantics: cratestack#915's five questions

### Q1. Flat merge or namespaced? **Flat.**

An imported file's declarations join the importer's single namespace. A collision is a hard error
naming both files (M13). There is no `as`, `show` or `hide`.

*Rejected: namespaced `import "common.cstack" as common`.* Every name a schema declares becomes a
global identifier in four generated surfaces that have no namespaces of their own: the Rust module
`cratestack_schema::types::Money`, the Dart and TypeScript packages, table and route names, and
the RPC op IDs `model.<Model>.<verb>`. A namespace would therefore need a mangling scheme in every
emitter and a wire-visible spelling, and every reference site would change. Collisions in a
flat namespace are loud, they name both files, and they are fixed by renaming. Adding `as` later
is non-breaking: `import "x" as y` is a parse error today, so no schema can already depend on it.

### Q2. Transitive? **Yes.**

If `main` imports `common` and `common` imports `base`, `main` sees `base`'s declarations.

*Rejected: non-transitive with a re-export mechanism.* The generated surface has to contain
everything an included declaration references (`common`'s `Money` refers to `base`'s `Currency`,
so `Currency` is generated for `main` regardless). Non-transitivity could therefore only be a
name-resolution lint, not a boundary. In a flat namespace a hidden `base` name still collides with
`main`'s own, so hiding it would prevent no conflict. It would also need a second feature
(re-export). cratestack#913 allowed omitting re-export syntax unless this question required it,
and it does not.

### Q3. Diamonds: what "canonical" means. **`std::fs::canonicalize`.**

A file is identified by `std::fs::canonicalize` of its resolved path, which resolves `.`, `..`
and every symlink. `main` importing `a` and `b`, which both import `c`, loads `c` once, whatever
spelling each import used (`../c.cstack`, `./x/../c.cstack`, a symlink to it). Two **different**
files with identical contents are two files, and collide (M13). Hard links have distinct
canonical paths and are treated the same way. That is documented, not worked around.

**One resolution rule.** A directive's path is resolved against the directory of the canonical
path of the file that holds it, and the result is canonicalized by the operating system; `..` is
never folded away lexically first. A file reached through a symlink therefore resolves its own
`import`s and `part`s from where it really is, whichever spelling loaded it first, so reordering
imports cannot change the outcome. Consequently a symlinked part's `part of` is written relative
to its real location. Paths shown in messages and code frames are the root's path as given, and
every other file as its real location relative to the root's real directory, joined to the
directory the root was given by (the root's real directory when the root is itself a symlink), so
a displayed path always names a file that exists. The macros take each `include_str!` path from
the path actually read, not from the macro literal.

When the source comes from an editor buffer that is not on disk yet (the LSP, §12), the key falls
back to the absolute, lexically normalized path (`std::path::absolute`, then `.`/`..` folded).

*Rejected: dedupe by the path as written.* Two spellings of one file would resolve twice and
report duplicate declarations that look like the author's mistake (cratestack#920's stated risk).

### Q4. Cycles. **Rejected, with the whole cycle.**

Only imports can form a cycle. Parts are one level deep and point at their root. A cycle is an
error at the import line that closes it, listing every unit on the way:

```text
import cycle: `schema.cstack` → `../shared/a.cstack` → `../shared/b.cstack` → `../shared/a.cstack`
```

A unit whose import line sits in one of its parts is written with that part:
`` `schema.cstack` (part `models/post.cstack`) → `schema.cstack` ``. A part importing its own root
is such a cycle.

### Q5. Do imported `model`s create tables? **No. An import carries `type`, `enum` and `mixin` only.**

Decided under delegation by the maintainer, 2026-11-08. cratestack#915 reserved this question for
the maintainer, who delegated it.

An imported file, and every part of an imported root, may declare `type`, `enum` and `mixin`
(and the directives). Anything that creates runtime surface or ownership is refused with M12:
`model`, `view`, every kind of `procedure`, `topic`, `query`, the root-only singletons and
`extension`. To split one schema's models across files, use `part`.

What an import *does* create: imported types, enums and mixins are full members of the merged
schema and generate exactly as if they were written locally. Two services importing
`money.cstack` each generate their own `Money` struct and the two are wire-compatible because they
have the same shape. An imported enum used by a model column is materialized in the importer's own
migrations (a `TEXT` + `CHECK` or `SMALLINT` column, cratestack#228). The import shares the shape,
and the importer owns the storage. A mixin's fields are copied into the importer's model by the
`@use` expansion, which already treats them as local fields (`reference/mixins`).

*Rejected: imported models are full models, so every importer owns its own copy of the table.*
The use case named in cratestack#913 is "share model and type definitions so the copies stop
drifting". For value shapes, `type`/`enum`/`mixin` serve it fully. For tables it backfires: two
services each owning a `users` table generated from one file is two databases whose schemas are
coupled through a third file. A change to `common.cstack` becomes a migration in every importer,
the same day, with policies (`@@allow`) and routes (REST, RPC, MCP) also copied into services that
may not want them. That is the shared-database coupling microservices are split to avoid, made
easy. The rule is also reversible in one direction only: allowing models in imports later is
non-breaking, and forbidding them after schemas depend on them would be a breaking change. So the
narrow rule ships first.

*Also rejected: imported models as references with no table.* That would need a third model mode
(no DDL, no routes, but generated struct and relations into a table this service does not own)
and a story for `@relation` across databases. That is a feature of its own, not a default.

## 6. Path resolution

- The macro argument stays **`CARGO_MANIFEST_DIR`-relative**, unchanged
  (`include/parse.rs:19`). A CLI `--schema` stays relative to the working directory.
- Every `part`, `part of` and `import` path is **relative to the directory of the file that
  contains the directive.** `part of "../schema.cstack"` in `schema/task.cstack` names
  `schema.cstack`.
- A path must be relative, use `/` as its only separator and end in `.cstack` (M15). An absolute
  path would make a schema non-portable between machines, a backslash would make it
  non-portable between platforms, and the suffix rule catches `part "models/post"` at the
  directive rather than as an unreadable file.
- *Rejected: every path relative to `CARGO_MANIFEST_DIR`.* A shared file imported from two crates
  would need a different spelling for its own nested imports in each, and a part could not be
  moved with its root.

Files outside the crate directory (`import "../../shared/money.cstack"`) work inside a workspace.
`cargo publish` packages only the crate directory, so a published crate must keep its schema
files inside it. This is the same constraint `include_str!` has always had.

## 7. Load order (cratestack#917)

The order is defined, documented and independent of filesystem iteration. Nothing ever lists a
directory.

1. The root file.
2. The root's parts, in the order the root's `part` lines appear.
3. Then, for each file loaded in steps 1–2, in that order, each of its `import` lines in order:
   the imported unit (its root, then its parts, then its imports, by these same rules),
   unless that unit was already loaded (same canonical path). In that case it is skipped and
   loaded only once.

This is pre-order: the root's own declarations come first, so adding an import never moves the
root's declarations in generated output.

Declarations are merged per kind (models, types, enums, mixins, views, queries, procedures,
subscription procedures, topics) by appending in load order, keeping each file's internal order.
`declared_extensions` is the union. `datasource`, `auth`, `mcp` and `transport` come from the root,
because §4 forbids them elsewhere. Mixins are expanded **after** the merge, so a model in one file
can `@use` a mixin from another.

**The single-file equivalent** of a split schema is defined as: every file in load order,
concatenated, with its `part`, `part of` and `import` lines removed. §8's golden test holds
the implementation to that definition. The fixture's concatenated file is written by hand, never
generated by the code under test.

## 8. IR identity (cratestack#917)

Two IRs are **identical** when their `Schema` values are equal after every `SourceSpan` is
zeroed. In tests that means byte-equality of `serde_json::to_vec_pretty`, and
`cratestack print-ir --no-spans` prints the same form for people. Source positions are excluded
because they are positions in different texts by definition. Nothing else is excluded:
declaration order, `///` docs and attribute text all count.

Zeroing spans is `cratestack_core::schema::without_source_positions(&Schema) -> Schema`. It
serializes the `Schema`, sets every object shaped exactly `{start, end, line}` to zeros, and
deserializes the result. `SourceSpan` is the only IR type with that shape, and a unit test pins
this.

Consequences of identity, each asserted by the golden test, not assumed:

- `schema_digest` is equal, so `SCHEMA_SHA256` is unchanged by splitting.
- `cratestack_migrate::project` is equal, so `migrate diff` against a committed snapshot produces
  nothing after a split.
- Generated code is equal, because no generator reads a span (§1).

## 9. One address space for spans, and diagnostics that name both files

**Spans.** Each loaded file gets a `base` offset: `base₀ = 0` and `baseᵢ₊₁ = baseᵢ + lenᵢ + 1`.
The one-byte gap keeps a span at the end of one file from being attributed to the next. Each file
is parsed with its lines counted from `base` (`collect_lines_at(source, base)`), so every
`SourceSpan` in a merged `Schema` has offsets that are unique across the schema. `line` stays the
line **within the declaring file**. `LoadedSchema::locate(offset)` maps a global offset to
`(file, local offset)`. For a single-file schema `base = 0`, so every span is byte-identical to
today's.

*Rejected: a file id inside `SourceSpan`.* That would change a `Copy` type built by hand at 73
sites across six crates and add a field to every serialized span of every existing schema. The
address space changes no type and no existing value.

**Errors.** Every `SchemaError` produced while loading or validating a multi-file schema carries
global offsets until the loader's boundary, which attributes it once: it picks the file containing
`span.start`, rebases the span to that file, and attaches that file's display path and source.
That is the same job `SchemaError::with_file` does for one file today. `span()`, `line()`, `file()`
and `render()` are therefore always local to the error's own file.

**Naming both files.** Errors that involve two files say so in the message text, and also carry a
**related location**: `SchemaError::related() -> &[RelatedSpan]`, each with its own file, local
span, line and label. `render()` draws it as a second ariadne label in its own file's code frame.
`cratestack check --format json` adds `"related": [{ "file", "line", "start", "end", "message" }]`
to a diagnostic only when there is one. The LSP maps it to `Diagnostic.relatedInformation`. An
error with no related location renders and serializes byte-identically to today's.

**Paths in messages** are relative to the root file's directory, `/`-separated. The root itself is
shown by its file name. `SchemaError::file()` (the code frame's title) keeps the full display path.

### The messages

Quoted exactly. Docs, skills and tests quote these strings, so a wording change is a breaking
change for them.

| # | When | Message | Primary span; related |
|---|---|---|---|
| M1 | a listed part has no `part of` | ``` `{part}` is listed as a part of `{root}`, but does not declare `part of`; add `part of "{back}"` to `{part}` ``` | the root's `part` line |
| M2 | a listed part names another root | ``` `{part}` is listed as a part of `{root}`, but declares itself a part of `{other}` ``` | the part's `part of` line; the root's `part` line |
| M3 | a part names a root that does not list it (reachable from `check` and the LSP, §11, §12) | ``` `{part}` declares itself a part of `{root}`, but `{root}` does not list it; add `part "{rel}"` to `{root}` ``` | the `part of` line |
| M4 | a part is given where a root is required | ``` `{file}` is a part of `{root}`, not a schema root; point the macro or command at `{root}` ``` (`{file}` is the path as given; `{root}` is the root resolved against it, so it works from the same directory) | the `part of` line |
| M5 | a root-only singleton in a part | ``` `{kind}` belongs in the root schema (`{root}`), not in a part ``` (cratestack#993's wording) | the declaration |
| M6 | a part lists a part | ``` a part cannot list parts of its own; move `part "{path}"` to the root schema (`{root}`) ``` | the `part` line |
| M7 | two `part of` in one file | ``` duplicate `part of` (first declared on line {n}) ``` (cratestack#992's wording) | the second line |
| M8 | one file listed twice as a part | ``` `{path}` is already listed as a part on line {n} ``` | the second `part` line |
| M9 | a root lists itself | ``` `{root}` lists itself as a part ``` | the `part` line |
| M10 | an import names a part | ``` cannot import `{file}`: it is a part of `{root}`, and a part is locked to its root; import a file that does not declare `part of` instead ``` | the `import` line; the part's `part of` line |
| M11 | an import cycle | ``` import cycle: `{a}` → `{b}` → … → `{a}` ``` | the `import` line closing the cycle |
| M12 | a disallowed declaration in an import | ``` `{kind} {name}` cannot come from an import: `{file}` is imported by `{importer}`, and an imported file may declare only `type`, `enum` and `mixin`; use `part` to split one schema across files ``` (`{kind}` alone for `transport` / `mcp`) | the declaration; the `import` line |
| M13 | one name in two files | ``` `{name}` is declared twice: `{kind_a} {name}` in `{file_a}` (line {la}) and `{kind_b} {name}` in `{file_b}` (line {lb}) ``` | the later name; the earlier name |
| M14 | a listed file cannot be read | ``` cannot read `{path}` (listed on line {n} of `{file}`): {io error} ``` | the directive line |
| M15 | a bad path | ``` `{directive}` path `{path}` must be a relative path written with `/` and ending in `.cstack` ``` | the path |
| M16 | a directive without a quoted path | ``` expected a quoted path after `{directive}`, like `{directive} "models/post.cstack"` ``` | the line |
| M17 | a directive given to a single-source entry point (`parse_schema`, `parse_schema_named`, `parse_schema_diagnostics`, `parse_schema_unvalidated`) | ``` `{directive}` needs the schema to be loaded from a file, because "{path}" is resolved against that file's directory; use `parse_schema_file` or `cratestack check --schema <root>` ``` | the directive line |

The root file being unreadable keeps today's text exactly: `failed to read schema file {path}: {error}`.

M13 applies only **across** files, and only within the namespaces where one file already refuses
a repeat. Those are: `type`/`enum`/`model`/`mixin`/`auth` (`validate/type_names.rs`
`collect_type_names`), procedures together with subscription procedures (`validate/mod.rs:140-151`,
`validate/subscriptions.rs:64`), queries (`validate/queries.rs:70`), topics
(`validate/subscriptions.rs:102`), and views among themselves (`validate/views.rs:54-58`). A repeat within
one file still gets today's message, so single-file diagnostics do not change. A test enforces
the invariant that the loader never refuses a cross-file pair that one concatenated file would
accept. Splitting never turns a valid schema invalid.

The loader's structural checks (M1–M16, every file's syntax) are a hard stop: the first one is the
only error reported. Validation of the merged schema then runs in the existing stages, so
`load_schema_diagnostics` reports every independent validation error, from **several files in one
run** (cratestack#911).

## 10. `SCHEMA_SOURCE`, `SCHEMA_FILES`, `SCHEMA_SHA256` and rebuild tracking (cratestack#919)

- **`SCHEMA_SOURCE: &str` keeps its type and meaning: the root file's text**, still
  `include_str!` of the root. For a single-file schema it is byte-identical. For a split schema
  it is the root's text alone. A concatenation was rejected because it would invalidate every
  span the parser produced. A new type was rejected because it is a public API break for the
  one-file case that would gain nothing from it. Nothing in the repository reads `SCHEMA_SOURCE`
  (grep at `a6f37c2`).
- **`SCHEMA_FILES: &[(&str, &str)]`**, `(path, contents)` per file in load order, root first, is
  emitted **only when the schema spans more than one file**. Each path is relative to
  `CARGO_MANIFEST_DIR`, `/`-separated, lexically normalized. Each contents entry is
  `include_str!(<absolute path>)`. Those `include_str!`s are what register every part and
  import, transitively, as rebuild dependencies of the consuming crate. Emitting it for one file
  would change the generated output of every existing schema, which the epic rules out
  ("Out: any change to generated output for a schema that stays in one file").
- **`SCHEMA_SHA256` / `SCHEMA_SHA256_BYTES` are unchanged in definition**: `schema_digest` of the
  merged `Schema`. It covers every file, by construction. Its bytes do not depend on load order,
  because the canonical form sorts declarations, so a split does not change it (§8). The drift
  path is proven by making a part drift (§15, `schema_fingerprint_multi_file`): the server built
  from the original files logs the mismatch warning for a header carrying the drifted
  schema's digest, and stays silent for the matching one.
- `SCHEMA_PATH` is unchanged: the root's macro literal.

Same in all three macros.

**Why additions rebuild.** A new part is useful only once the root lists it. Listing it is an
edit to the root, which is tracked through `SCHEMA_SOURCE`, so the macro re-expands, reads the new
part, and emits an `include_str!` for it. A new `import` likewise edits a tracked file. A file that
declares `part of` but that no root lists is not read by any build, and that is by design. It is
reported as M3 by `cratestack check` on that file and by the LSP when it is opened. A build cannot
report it without globbing, and globbing is the mechanism the epic rejected. This is the one gap
in rebuild tracking and it is stated, not hidden. §15 demonstrates the tracking with a real
`cargo` run, never by inspection.

A file deleted from disk while still listed makes the dependency missing, so cargo re-runs the
macro, which fails with M14. A failed expansion is always re-run on the next build.

## 11. CLI (cratestack#914)

- `cratestack_parser::parse_schema_file` loads the whole graph. Every command that takes
  `--schema` (`check`, `print-ir`, `generate-dart`, `generate-typescript`, `generate-wiremock`,
  `migrate diff`, `migrate baseline`, `diff <old> <new>`, `contract …`) goes through it or
  through `cli_support::parse_schema_or_render`, so all of them resolve the full graph. Studio's
  `workspace/builder.rs` switches from `parse_schema_named(read)` to `parse_schema_file` for the
  same reason.
- **Those commands take the root.** Given a part they fail with M4. **`check` is the exception:**
  given a part, it follows `part of` to the root, checks the whole schema, and says so
  (cratestack#918: "`check` on a part alone says it is a part of X"):

  ```text
  $ cratestack check --schema schema/task.cstack
  `schema/task.cstack` is a part of `schema.cstack`; checked the whole schema
  schema OK: schema.cstack
  ```

  If the root does not list it, that is M3, exit 1. In `--format json` the payload keeps `schema`
  as given and adds `"root"` only when it differs.
- **`check --list-files`** reports the resolved file set after `schema OK`, one per line with its
  role (`root`, `part`, `import`, `import part`). In JSON, it adds
  `"files": [{ "path", "role" }]`. Without the flag the output is byte-identical to today's.
- **`print-ir --no-spans`** prints `without_source_positions` of the IR, the form §8 compares.
  Without the flag, unchanged.

## 12. LSP (cratestack#921)

- **Analysis follows the graph.** On open or change of a document that is a file, the server
  loads the schema containing it, with `follow_part_of` (§14). It uses an overlay of every open
  buffer, keyed by canonical path, and falls back to disk for the rest. So unsaved edits in any
  open file are what is checked.
- **Diagnostics are attributed to their file** and published per file for every file in the
  graph, including files that are not open. A file that had diagnostics and now has none gets an
  empty publish, so stale errors clear. Related locations become `relatedInformation`.
- **A part opened alone** is analyzed through its root, so it shows the whole schema's real
  diagnostics, not "missing datasource". When the root cannot be loaded, the single diagnostic
  says so: M3 if the root does not list the part, M14 if the root cannot be read. That is the
  "part of X" report cratestack#921 asks for, and it appears exactly when the part cannot be
  checked in context.
- **Editing any file refreshes the whole schema**: the server remembers each root's member files
  and re-analyzes every root whose graph contains the edited file (an edit to `money.cstack`
  refreshes every open importer).
- **Go-to-definition crosses files**: a definition span is global. The server locates its file and
  answers with that file's URI and a range in that file's text.
- **Completion offers the whole graph's declarations**, because it reads the merged schema.
- **Hover, document symbols, semantic tokens, references and highlights** work on the merged
  schema but answer only with spans inside the requesting file. References and highlights are
  therefore file-local in a multi-file schema. *Deferred*, follow-up issue to be filed with the PR.
- **Rename is refused in a schema that spans more than one file**: `prepareRename` returns
  nothing and `rename` returns the error `rename is not available yet in a schema split across
  files`. A single-file rename of a name used in another part would silently break that part.
  Cross-file rename is *deferred*, follow-up issue.
- **Rename is also refused in a file another schema imports or lists as a `part`**, even when
  that file has no directive of its own and was opened alone (a shared `common.cstack` holding
  only `type` / `enum` / `mixin`). The server records the client's workspace roots at
  `initialize` (`workspaceFolders`, else the deprecated `rootUri`). On `prepareRename` / `rename`
  it walks them for `*.cstack` files (skipping hidden directories, `target` and `node_modules`),
  reads each from its open buffer if there is one, else from disk, extracts the `import` /
  `part` targets with `cratestack_parser::directive_targets`, resolves them as the loader does
  (§6) and compares canonical paths, so a symlinked spelling matches. The walk runs on a blocking
  thread. Past 10,000 files or directories the search gives up and the rename is refused.
  *Limit:* with no workspace roots, or an importer outside them, nothing can be known and rename
  behaves as before.
- The completion detail for `part` / `import` drops "the declaration is not implemented yet"
  (`completion.rs:85`).
- A document with no file path (an untitled buffer) keeps today's single-text analysis.
- **Cost.** A keystroke re-parses the whole graph. Schemas are small and the parse is linear, so
  this is accepted for now. Revisit with per-file caching keyed by text if a large schema makes
  it noticeable.

## 13. Studio, facades and the other consumers

Studio loads through `parse_schema_file` (§11). The facades already re-export
`parse_schema_file`, which is now multi-file. They do not re-export the loader types (no facade
surface change). `cratestack-migrate`, the clients and WireMock take a `Schema` and need nothing.

## 14. Parser API

```rust
// crates/cratestack-parser (re-exported at the crate root)
pub struct PathDirective { pub path: String, pub path_span: SourceSpan, pub span: SourceSpan }
pub struct FileDirectives {
    pub parts: Vec<PathDirective>,          // `part "…"`, in source order
    pub part_of: Option<PathDirective>,     // `part of "…"`
    pub imports: Vec<PathDirective>,        // `import "…"`, in source order
}

pub trait SchemaSources {                   // where file text comes from
    fn read(&self, path: &Path) -> std::io::Result<String>;
}
pub struct FsSources;                       // std::fs::read_to_string
pub struct OverlaySources { /* canonical path -> text, then optionally disk */ }
pub fn canonical_schema_path(path: &Path) -> PathBuf;  // §5 Q3

#[derive(Default)]
pub struct LoadOptions<'a> {
    pub follow_part_of: bool,               // `check` and the LSP only
    pub sources: Option<&'a dyn SchemaSources>,  // default: FsSources
}
pub enum FileRole { Root, Part, Import, ImportPart }
pub struct SchemaFile {
    pub display: String,                    // path as shown in a code frame
    pub relative: String,                   // root-relative, as shown in messages
    pub path: PathBuf,                      // canonical
    pub source: Arc<str>,
    pub base: usize,                        // §9
    pub role: FileRole,
}
pub struct LoadedSchema { pub schema: Schema, pub files: Vec<SchemaFile> }
impl LoadedSchema {
    pub fn root(&self) -> &SchemaFile;
    pub fn locate(&self, offset: usize) -> Option<(&SchemaFile, usize)>;
    pub fn is_single_file(&self) -> bool;
}
pub fn load_schema(path: impl AsRef<Path>, options: &LoadOptions<'_>) -> Result<LoadedSchema, SchemaError>;
pub struct LoadDiagnostics { pub schema: Option<LoadedSchema>, pub errors: Vec<SchemaError>, pub files: Vec<PathBuf> }
pub fn load_schema_diagnostics(path: impl AsRef<Path>, options: &LoadOptions<'_>) -> LoadDiagnostics;

// unchanged signature, now `load_schema(path, &Default::default()).map(|l| l.schema)`
pub fn parse_schema_file(path: impl AsRef<Path>) -> Result<Schema, SchemaError>;

// SchemaError gains:
pub struct RelatedSpan { /* message, span, line, file, source_text */ }
impl SchemaError { pub fn related(&self) -> &[RelatedSpan]; }
```

`LoadDiagnostics::files` lists every file read, including when loading stopped early, so the LSP
can clear diagnostics on each. For a schema with no directives, `load_schema(path)` returns
exactly what `parse_schema_named(path, read(path))` returns, spans included. The census test
asserts that over every committed single-file `.cstack` (§15).

## 15. Proof, mapped to the epic's success metrics

| Metric (cratestack#910) | Test |
|---|---|
| 1. A split schema and its concatenated equivalent produce identical IR | `crates/cratestack-parser/tests/multi_file_ir_identity.rs`: a fixture with a root, three parts (one with a `@use` of a mixin from another file), an import with its own part, and a diamond, against a hand-written `concatenated.cstack`. It asserts IR identity (§8), equal `schema_digest`, and (in `crates/cratestack-migrate/tests/multi_file_projection.rs`) equal projections. A perturbation test swaps two declarations across parts in a second fixture and asserts **inequality**, so the test is shown to fail when it should. |
| 2. Adding a part triggers a rebuild, by actually adding one | `.ci/multi-file-rebuild-check.sh` (`just verify-multi-file-rebuild`, CI job `multi-file rebuild`). It drives real `cargo` builds of `cratestack-sqlite`'s `multi_file_schema` test target and reads cargo's `"fresh"` flag. A no-op build is fresh. Editing the root, a part, an import and an import's part each rebuild it. Adding a new part file and listing it rebuilds it, the dep-info lists the new file, and a test run sees the new type in `TYPES`. An unlisted `part of` file does **not** rebuild it (the §10 trade-off, pinned). |
| 3. A cycle, a cross-part duplicate, and an import of a part each name both files | `crates/cratestack-parser/tests/multi_file_errors.rs`: one test per M-message, asserting the exact text, the primary file and span, and the related file and span |
| 4. Existing single-file examples generate byte-identical output | `committed_schemas.rs`: `load_schema == parse_schema_named` (spans included) for every committed single-file schema. `SCHEMA_FILES` is not emitted for one file. `just regen-examples --check` stays clean after `react-vite-swr` is split, which proves the Dart and TypeScript generators produce identical bytes from the split schema. |
| #919 drift | `crates/cratestack-pg/tests/schema_fingerprint_multi_file.rs`: the server from the split fixture warns on the digest of a fixture whose part drifted, and is silent on the digest of the identical concatenated fixture (`tests/support/tracing_capture.rs`) |

## 16. The split example (cratestack#924)

`examples/react-vite-swr/schema.cstack` is split into the root shown in §2 and
`schema/board.cstack`, `schema/task.cstack` and `schema/procedures.cstack`, preserving the
per-kind order. The root path does not change, so every reference to it (the `justfile`'s
`regen-examples`, `ci.yml`, both example READMEs) still works. Both committed generated clients
(`examples/flutter-riverpod/client`, `examples/react-vite-swr/client`) must be unchanged:
`just regen-examples --check` is the CI check that proves it. The example's tests pin
`SCHEMA_SHA256` to the value recorded **before** the split.

## 17. Scope and tickets

**Closed by this work:** #915 (this document), #917, #918, #919, #920, #921, #924, and the
sub-tickets of #918: #991, #992, #993 (implemented, with #992's "cannot be checked on its own
yet" replaced by §11's check-through-root, and #991's "not supported yet" moot), and #990 as
*not planned, superseded* by `FileDirectives` (§2). #923 is closed citing #929, which landed the
highlighting, plus a tokenization test of the shipped grammar standing in for the screenshot. The
stories #911, #912, #913, #914 and the epic #910 close when their children do. #916 and #922
were closed earlier.

**Deferred, each non-breaking to add later:**

| Deferred | Why it can wait |
|---|---|
| `import … as ns`, `show` / `hide` | Q1: today a parse error, so free to add |
| Models, views, procedures through `import` | Q5: relaxing the rule is additive |
| Cross-file LSP rename and references | Rename is refused rather than wrong (§12) |
| Reporting more than one syntax error per run across files | Parsing has no recovery today, in one file either |
| The `tree-sitter-cstack` grammar (sibling repository) | Not in this repository |
| Package or registry imports (`import "@scope/pkg"`) | Out of the epic's scope |
| Detecting an unlisted `part of` file during a build | Impossible without globbing (§10) |
