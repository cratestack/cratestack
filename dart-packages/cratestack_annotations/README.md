# cratestack_annotations

Annotations consumed by [CrateStack](https://cratestack.dev)'s Dart code
generators. Runtime-only and dependency-free.

## Why this is separate from `cratestack_builder`

A generated CrateStack client lists this package under `dependencies:` — the
annotation is referenced by the emitted source. Pub resolves a package's own
`dependencies:` transitively into the consumer's graph, so folding the
generator in here would put `analyzer`, `build` and `source_gen` into the
runtime graph of every Flutter app consuming a generated client, and drag
that app into the same analyzer-version negotiation the codegen toolchain
has to do.

Same split as `json_annotation`/`json_serializable`, and the one the
generated riverpod-preset pubspec already relies on for
`dart_mappable`/`dart_mappable_builder`.

## Usage

```dart
import 'package:cratestack_annotations/cratestack_annotations.dart';

part 'models.builder.dart';

@CratestackBuilder()
class Board {
  const Board({required this.name, this.tags});
  final String name;
  final List<String>? tags;
}
```

Then add `cratestack_builder` and `build_runner` as dev dependencies and run
`dart run build_runner build`. See that package for what gets generated.

### `listDefaults`

`@CratestackBuilder(listDefaults: false)` makes an unset list field build as
`null` rather than `[]`. CrateStack emits this on patch inputs
(`Update{Model}Input`), where "untouched" must stay distinguishable from
"explicitly set to empty".

This is the one thing the generator cannot work out for itself: a projection
model's list field and a patch input's list field emit byte-identical Dart,
so the distinction has to come from whoever applies the annotation.
