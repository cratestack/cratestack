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
          '  final Set<String> touchFlagFields;\n'
          '  final Set<String> nonDefaultingListFields;\n'
          '  const CratestackBuilder({\n'
          '    this.listDefaults = true,\n'
          '    this.touchFlagFields = const {},\n'
          '    this.nonDefaultingListFields = const {},\n'
          '  });\n'
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
    await expectGenerated(
        '''
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
    await expectGenerated(
        '''
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
    await expectGenerated(
        '''
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

  test(
      'an optional non-nullable field with a default value falls back to '
      'that default, not a bare (nullable) backing field', () async {
    // Regression for a real compile error hit by `cratestack-client-dart`'s
    // own generated `Update{Model}Input` classes: the `{field}IsSet` touch
    // flag for a nullable Patch field
    // (`crates/cratestack-client-dart/src/patch_touch.rs`) is `bool
    // {field}IsSet = false` — optional (no `required`), but *not*
    // nullable-typed. The backing field is still forced nullable
    // (`backingType`), so passing it straight through — what an earlier
    // revision of this generator did — is `bool? ` where `bool` is
    // expected, a real `argument_type_not_assignable` error whenever the
    // field was never explicitly set via the builder. `noteIsSet` here
    // stands in for that touch flag without pulling in the patch-touch
    // machinery itself — the shape (optional, non-nullable, defaulted) is
    // all this generator can see either way.
    await expectGenerated(
        '''
@CratestackBuilder(listDefaults: false, touchFlagFields: {'note'})
class UpdateGadgetInput {
  const UpdateGadgetInput({this.note, this.noteIsSet = false});
  final String? note;
  final bool noteIsSet;
}
''',
        allOf(
          contains('noteIsSet: _noteIsSet ?? false'),
          isNot(contains('noteIsSet: _noteIsSet,')),
        ));
  });

  test(
      "a field named in touchFlagFields gets a setter that also marks its "
      'sibling {field}IsSet touch flag touched', () async {
    // Regression for a real, silent behavior break hit by
    // `cratestack-client-dart`'s own generated `Update{Model}Input`
    // classes (cratestack#663): the OLD inline `model_builder_class.dart.j2`
    // template linked a field's setter to its `{field}IsSet` companion by
    // construction (both were driven off one shared internal tracking
    // bool). This generator treats every constructor parameter
    // independently, so without recovering that link, `.note(value)` alone
    // left `noteIsSet`'s backing field untouched and `build()` silently
    // computed `noteIsSet: false` — indistinguishable from "never
    // touched", which broke the "explicitly cleared to null" wire
    // representation cratestack#663 exists for. Caught empirically by
    // `crates/cratestack-client-dart/tests/fixtures/
    // builder_edge_cases_patch_test.dart`'s `an explicitly-cleared
    // nullable field serializes as an explicit null` under `just
    // verify-dart`, not by any text-level assertion.
    //
    // The link is supplied EXPLICITLY via `touchFlagFields: {'note'}`, not
    // recovered by matching a `bool` field named `noteIsSet` — a name-shape
    // heuristic fires on any ordinary field shaped that way too
    // (`cratestack-parser`'s `tests_patch_touch_flag_collisions.rs`
    // deliberately accepts a non-nullable `weight` beside an unrelated
    // `weightIsSet` field), which the two tests below both pin: neither
    // fires the linkage despite matching the naming shape, because neither
    // annotation lists the field in `touchFlagFields`.
    await expectGenerated(
        '''
@CratestackBuilder(listDefaults: false, touchFlagFields: {'note'})
class UpdateGadgetInput {
  const UpdateGadgetInput({this.note, this.noteIsSet = false});
  final String? note;
  final bool noteIsSet;
}
''',
        contains(
          '  UpdateGadgetInputBuilder note(String? value) {\n'
          '    _note = value;\n'
          '    _noteIsSet = true;\n'
          '    return this;\n'
          '  }\n',
        ));
  });

  test('a {field}IsSet touch flag gets NO setter of its own', () async {
    // Corrected from asserting the opposite. An independent
    // `noteIsSet(bool)` setter lets a caller write
    // `.note('x').noteIsSet(false)` and build a patch that claims the field
    // is untouched while carrying a value — order-dependent nonsense. The
    // inline builder this generator replaced made that unrepresentable by
    // keeping its tracking bool private, and parity measurement against it
    // is what surfaced the difference.
    //
    // The flag is derived state: `note`'s setter marks it (tested above)
    // and `build()` defaults it to `false`. Suppression is computed from
    // `touchFlagFields` — naming `note` already implies `noteIsSet` — so
    // this needs no additional annotation argument.
    await expectGenerated('''
@CratestackBuilder(listDefaults: false, touchFlagFields: {'note'})
class UpdateGadgetInput {
  const UpdateGadgetInput({this.note, this.noteIsSet = false});
  final String? note;
  final bool noteIsSet;
}
''',
        allOf(
          isNot(contains('UpdateGadgetInputBuilder noteIsSet(bool value) {')),
          // still constructed, defaulted, and still linked to `note`
          contains('noteIsSet: _noteIsSet ?? false'),
        ));
  });

  test(
      'a bool field shaped like a touch flag is NOT linked unless named in '
      'touchFlagFields', () async {
    // The false-positive `weight`/`weightIsSet` shape `cratestack-parser`
    // deliberately accepts as two unrelated fields (`weight` non-nullable,
    // so Rust never synthesizes a touch flag for it at all) — an earlier,
    // name-shape-based revision of this generator linked them anyway.
    await expectGenerated('''
@CratestackBuilder()
class Widget {
  const Widget({required this.id, required this.weight, required this.weightIsSet});
  final int id;
  final int weight;
  final bool weightIsSet;
}
''', isNot(contains('_weightIsSet = true')));
  });

  test(
      'nonDefaultingListFields keeps an unset list null and suppresses its '
      'append setter', () async {
    // Mirrors a to-many relation field on a generated model class (issue
    // #661): Rust's own model builder drops relation fields entirely, so
    // this field must NOT default to `[]` or get an `addPosts` setter,
    // even though `listDefaults` is `true` for every other list field on
    // the same class.
    await expectGenerated(
        '''
@CratestackBuilder(nonDefaultingListFields: {'posts'})
class Author {
  const Author({this.posts});
  final List<String>? posts;
}
''',
        allOf(
          // No trailing punctuation assumed — see the `listDefaults: false`
          // test above for why (`dart format` may collapse a single-arg
          // call onto one line).
          contains('posts: _posts'),
          isNot(contains('?? <String>[]')),
          isNot(contains('addPosts')),
        ));
  });

  test('no static builder() factory is emitted', () async {
    // Dart puts static and instance members in one namespace, so a static
    // `Gadget.builder()` would collide with the `builder` field below.
    // `GadgetBuilder()` is the only entry point.
    await expectGenerated(
        '''
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
