# Changelog

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

### `CratestackBuilder` gained `touchFlagFields` and `nonDefaultingListFields`

Both are additive, defaulting to an empty `Set<String>`, so no existing `@CratestackBuilder(...)` call
site needs to change. `package:cratestack_builder` 0.8.7 reads them to replace a by-name heuristic that
collided with ordinary schema fields (`touchFlagFields`) and to stop defaulting an unset to-many relation
field on a generated model class to `[]` (`nonDefaultingListFields`) — see that package's own CHANGELOG
for the full rationale.

## 0.8.5

Initial release. Provides `@CratestackBuilder`, consumed by
`package:cratestack_builder`.
