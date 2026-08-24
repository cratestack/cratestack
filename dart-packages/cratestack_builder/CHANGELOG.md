# Changelog

## Unreleased

## 0.8.11 (2026-08-24)

`pubspec.yaml` no longer carries a `dependency_overrides` block pointing `cratestack_annotations` at
the sibling directory (cratestack#714). It was a bootstrap affordance from the phase where the
`touchFlagFields`/`nonDefaultingListFields` arguments existed only in unpublished source;
`cratestack_annotations` 0.8.10 is live on pub.dev, so this package now resolves it from the registry
like any other consumer. That was confirmed at the time by checking the *resolved* path
(`.pub-cache/hosted/pub.dev/cratestack_annotations-0.8.10`) rather than by reading the manifest — a
stale override is invisible from the manifest alone.

The declared constraint is unchanged at `^0.8.8`, deliberately: it names the earliest annotation
surface this generator uses, not the current version, and caret on a `0.x` version already pins the
second component, so `^0.8.8` resolves 0.8.10.

No generator behaviour changed — nothing under `lib/` was touched in this range.

That placeholder is what 0.8.11 actually shipped: the release went out with the raw seed still in
this file, because `main` has no required status checks and the gate that caught it could not block
the merge. This entry corrects the repository and every archive from 0.8.12 onward — pub.dev's
published 0.8.11 page keeps the seed text, since an uploaded archive is immutable.

## 0.8.10 (2026-08-23)

First release carrying the annotation arguments the CrateStack Dart generator needs, and the first
release of these packages that the repo's changelog tooling tracks — see below.

- `touchFlagFields` — names the fields that have a Rust-synthesized `{field}IsSet` sibling, so the
  generated setter marks it touched too. Explicit rather than recovered from the
  `{field}`/`{field}IsSet` name shape: a schema may legally declare an unrelated `bool` field ending
  in `IsSet`, and a name heuristic made the other field's setter silently clobber it.
- `nonDefaultingListFields` — names list fields that must NOT default to `[]` or gain an
  `add{Field}` setter: to-many relations on a projection model, and the synthesized
  `{Model}FindMany.orderBy`. There, `null` means "not included in the response" and `[]` means
  "included and empty".
- Fixes `argument_type_not_assignable: bool? -> bool` for an optional non-nullable defaulted field —
  exactly the `{field}IsSet` touch-flag shape — which the generator produced for its own output.

0.8.8 and 0.8.9 were never published; 0.8.6 carried no changes to these packages. Prior to this
release these CHANGELOGs were not in `.ci/changelog-files.sh`'s declared list, so the release
tooling never seeded or dated them and they silently fell behind `pubspec.yaml` — which is what
`dart pub publish` was warning about with "CHANGELOG.md doesn't mention current version".

## 0.8.7 (2026-08-23)

### Fix: a field's setter silently failed to mark its `{field}IsSet` touch flag touched

Any `@CratestackBuilder()`-annotated class carrying `cratestack-client-dart`'s `{field}`/`{field}IsSet`
touch-flag pair (issue #663 — a nullable Patch field needs a way to distinguish "untouched" from
"explicitly cleared to null" on the wire) generated a `{field}` setter that updated only `{field}`'s own
backing field, leaving `{field}IsSet` at its default. `build()` therefore computed `{field}IsSet: false`
even when the caller had explicitly called `.{field}(null)`, indistinguishable from never having touched
the field at all — silently reintroducing the exact bug issue #663 fixed, for every generated
`Update{Model}Input`'s nullable field, the moment issue #668 phase 2 moved builder generation out of
`cratestack-client-dart`. Caught by `crates/cratestack-client-dart/tests/fixtures/
builder_edge_cases_patch_test.dart`'s `an explicitly-cleared nullable field serializes as an explicit
null` under `just verify-dart` — a real, running package + `build_runner` + `flutter test` sequence, not
a text-level assertion.

Fixed by recovering the `{field}`/`{field}IsSet` link structurally (a `bool`-typed field named exactly
`{other}IsSet` is treated as `{other}`'s touch flag) and having `{field}`'s own setter also mark the
linked flag touched, mirroring the pre-#668 inline template's behavior.

**Superseded within this same release** — see "the structural touch-flag heuristic collided with
ordinary fields" below: the by-name heuristic this fix introduced was itself wrong and has been replaced
with an explicit `touchFlagFields` annotation argument.

### Fix: an optional non-nullable field with a default value crashed `build()`

Any `@CratestackBuilder()`-annotated class whose constructor has an optional (non-`required`), non-list,
non-nullable named parameter with a default value — the shape `cratestack-client-dart`'s own
`Update{Model}Input.{field}IsSet` touch flag uses (issue #663) — produced a `build()` that passed the
nullable backing field straight through, a real `argument_type_not_assignable` compile error whenever
the field was never explicitly set via the builder. Every generated `Update{Model}Input` with at least
one nullable field hit this the moment issue #668 phase 2 started annotating patch classes.

Fixed by falling back to the parameter's own recovered default (`FormalParameterElement.defaultValueCode`)
instead of the raw backing field, e.g. `noteIsSet: _noteIsSet ?? false`.

### Fix: the structural touch-flag heuristic collided with ordinary fields

The by-name heuristic added above — any `bool`-typed field whose identifier ends in `IsSet` is treated
as some other field's touch flag — fires on a schema that legitimately declares a standalone field shaped
that way. `cratestack-parser`'s `tests_patch_touch_flag_collisions.rs` deliberately accepts a
non-nullable `weight` beside an unrelated `weightIsSet` field (`weight` is non-nullable, so Rust
synthesizes no touch flag for it at all); the heuristic linked them anyway, so `.weight(5)` silently
overwrote whatever the caller had explicitly set via `.weightIsSet(false)`, order-dependently.

Fixed by replacing the heuristic with an explicit `touchFlagFields: Set<String>` argument on
`@CratestackBuilder(...)`, naming exactly the fields Rust actually synthesized a touch flag for. The
by-name recovery is gone — a hand-written class that wants the same linkage now states it explicitly.

### Fix: a to-many relation field on a model class defaulted to `[]` instead of staying `null`

`package:cratestack_builder` derives list-ness purely from `DartType.isDartCoreList`, which cannot
distinguish a scalar list field from a to-many relation field on a generated model class — the two are
structurally identical Dart (`final List<Post>? posts;`). Every list field on a non-patch class
(`listDefaults: true`) therefore defaulted an unset value to `[]` and gained an `add{Field}` setter,
including relation fields — conflating "this relation was not included in the response" with "included
and empty" (the exact cross-language divergence issue #661 exists to prevent), since Rust's own model
builder has no counterpart for a relation field at all (`scalar_model_fields` drops them).

Fixed by adding a `nonDefaultingListFields: Set<String>` argument on `@CratestackBuilder(...)`: field
identifiers to treat as non-list for builder purposes (no `add{Field}` setter, no `?? []` default) even
though `listDefaults` is `true` for the class as a whole.

## 0.8.5

Initial release. Generates a fluent `{Class}Builder` into a
`part '<file>.builder.dart'` for every class annotated with
`@CratestackBuilder` from `package:cratestack_annotations`.
