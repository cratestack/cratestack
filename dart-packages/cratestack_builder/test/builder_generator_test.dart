// Drives the builder over in-memory sources and asserts the emitted Dart.
//
// Deliberately tests the generator directly rather than through a consumer
// package: the interesting cases are ones where the generator must reach a
// conclusion the source does not state outright (required-but-nullable
// fields, patch vs projection list defaulting), and a consumer-level test
// can only observe the outcome, not which signal produced it.
import 'package:build/build.dart';
import 'package:build_test/build_test.dart';
import 'package:cratestack_builder/builder.dart';
import 'package:test/test.dart';

/// The annotation has to be supplied at its REAL URIs, mirroring the
/// package's actual structure. `GeneratorForAnnotation` matches on the
/// annotation's *declaring* library — which is `src/...`, not the barrel —
/// so a stub placed only at the barrel URI declares a different type as far
/// as the `TypeChecker` is concerned, and every input silently comes back
/// "no-op" with "Could not resolve annotation".
const _annotationSources = {
  'cratestack_annotations|lib/cratestack_annotations.dart':
      "export 'src/cratestack_builder_annotation.dart' show CratestackBuilder;\n",
  'cratestack_annotations|lib/src/cratestack_builder_annotation.dart':
      'class CratestackBuilder {\n'
          '  final bool listDefaults;\n'
          '  const CratestackBuilder({this.listDefaults = true});\n'
          '}\n',
};

/// Runs the builder over [source] and asserts the generated part against
/// [matcher].
///
/// Uses `testBuilder`'s own `outputs:` assertion rather than reading the
/// asset back afterwards — a generated output is not readable through the
/// reader once the build has completed.
Future<void> expectGenerated(String source, Matcher matcher) async {
  await testBuilder(
    cratestackBuilder(BuilderOptions.empty),
    {
      'a|lib/models.dart':
          "import 'package:cratestack_annotations/cratestack_annotations.dart';\n"
              "part 'models.builder.dart';\n\n$source",
      ..._annotationSources,
    },
    outputs: {'a|lib/models.builder.dart': decodedMatches(matcher)},
  );
}

const _gadgetWithList = '''
@CratestackBuilder()
class Gadget {
  const Gadget({this.tags});
  final List<String>? tags;
}
''';

void main() {
  test('PartBuilder emits the part-of header itself', () async {
    // The lean_builder prototype had to hand-prepend this and got it wrong;
    // PartBuilder owning it is much of why source_gen was chosen instead.
    await expectGenerated('''
@CratestackBuilder()
class Gadget {
  const Gadget({this.id});
  final int? id;
}
''', contains("part of 'models.dart';"));
  });

  test('a required field is enforced via a _set flag, not a null check',
      () async {
    // `meta` is required AND nullable — the case a naive `isNullable` check
    // gets wrong. Required-ness comes from `isRequiredNamed` on the
    // constructor parameter, so a nullable required field is still enforced
    // and an explicitly-set null still counts as set.
    await expectGenerated('''
@CratestackBuilder()
class Gadget {
  const Gadget({required this.meta});
  final Object? meta;
}
''',
        allOf(
          contains('_metaSet'),
          contains('Gadget.meta is required but was not set'),
        ));
  });

  test('a field named `build` gets a setBuild shim', () async {
    // Without the shim the field's setter collides with the terminal
    // build().
    await expectGenerated('''
@CratestackBuilder()
class Gadget {
  const Gadget({this.build});
  final String? build;
}
''',
        allOf(
          contains('setBuild'),
          isNot(contains('GadgetBuilder build(String? value)')),
        ));
  });

  test('listDefaults defaulting to true makes an unset list build as []',
      () async {
    await expectGenerated(
        _gadgetWithList, contains('tags: _tags ?? <String>[]'));
  });

  test('listDefaults: false leaves an unset patch list null', () async {
    // The field declaration below is byte-identical to `_gadgetWithList`.
    // The only difference is the annotation — which is precisely why the
    // flag has to exist: patch-ness is not recoverable from the source.
    await expectGenerated('''
@CratestackBuilder(listDefaults: false)
class UpdateGadgetInput {
  const UpdateGadgetInput({this.tags});
  final List<String>? tags;
}
''',
        allOf(
          // No trailing punctuation in either matcher: source_gen runs
          // `dart format` over its output, so whether this lands as
          // `tags: _tags,` on its own line or `(tags: _tags)` collapsed
          // onto one depends on the surrounding line width, not on the
          // generator. Asserting the pair — the field is passed through,
          // and no default is applied — pins the behaviour without pinning
          // the formatter.
          contains('tags: _tags'),
          isNot(contains('?? <String>[]')),
        ));
  });

  test('the append setter copies rather than mutating', () async {
    // Mutating in place throws on any non-growable list, including every
    // list a CrateStack client's own fromWire produces via
    // `.toList(growable: false)`. That shipped as a real blocker once.
    await expectGenerated(_gadgetWithList, contains('<String>[...?_tags]'));
  });

  test('no static builder() factory is emitted', () async {
    // Dart puts static and instance members in one namespace, so a static
    // `Gadget.builder()` would collide with the `builder` field below.
    // `GadgetBuilder()` is the only entry point.
    await expectGenerated('''
@CratestackBuilder()
class Gadget {
  const Gadget({this.id, this.builder});
  final int? id;
  final String? builder;
}
''',
        allOf(
          contains('class GadgetBuilder'),
          isNot(contains('static GadgetBuilder builder()')),
        ));
  });
}
