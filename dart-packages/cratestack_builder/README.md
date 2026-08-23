# cratestack_builder

`build_runner` generator that produces fluent builders for classes annotated
with `@CratestackBuilder` from
[`package:cratestack_annotations`](https://pub.dev/packages/cratestack_annotations).

Add it as a **dev** dependency; the annotation package is the runtime one.

```yaml
dependencies:
  cratestack_annotations: ^0.8.4

dev_dependencies:
  build_runner: ^2.15.0
  cratestack_builder: ^0.8.4
```

```
dart run build_runner build
```

## What it generates

For an annotated class, a `{Class}Builder` in a `part '<file>.builder.dart'`:

- one fluent setter per field, returning the builder
- `build()`, throwing `StateError` naming the class and field if a required
  field was never set — required-ness is read from `isRequiredNamed` on the
  constructor parameter, so a **required but nullable** field is still
  enforced, and an explicitly-set `null` still counts as set
- for list fields, an `add{Field}(item)` append setter alongside the bulk
  setter. It copies rather than mutating, so it works on non-growable lists
- a field named `build` gets a `setBuild` setter, so it does not collide
  with the terminal `build()`

There is deliberately no static `Class.builder()` factory: Dart puts static
and instance members in one namespace, so it would collide with any field
named `builder`. Construct `ClassBuilder()` directly.

## `@CratestackBuilder(...)`'s three arguments

Almost everything is recovered from the annotated class's Dart source itself.
Three pieces of information genuinely aren't, because two different source
shapes can be byte-identical yet need different generated behavior — so the
annotation carries them explicitly:

- `listDefaults` (`bool`, default `true`) — whether an unset list field
  builds as `[]` (the default) or stays `null`.
- `touchFlagFields` (`Set<String>`, default `{}`) — field identifiers that
  carry a sibling `{field}IsSet` `bool` field; that field's own setter also
  marks `{field}IsSet` touched.
- `nonDefaultingListFields` (`Set<String>`, default `{}`) — list-typed field
  identifiers to exclude from `listDefaults`'/`add{Field}` treatment even
  though the class as a whole wants it — e.g. a to-many relation field where
  "not included in the response" (`null`) must stay distinguishable from
  "included and empty" (`[]`).

## Analyzer constraint

The `analyzer` bound is `>=12.0.0 <13.0.0`, and the **upper bound is
load-bearing**. Under CrateStack's riverpod preset this builder runs in the
same `build_runner` pass as `riverpod_generator`, whose own constraint is
`analyzer ^12.0.0`; allowing 13.x makes `pub get` fail in that package
before codegen is reached. Raise it only alongside `riverpod_generator`.
