// issue #661: real, running proof (not just the text-level assertions in
// `tests/generator.rs::list_field_builder_defaults_to_empty_list_and_gains_an_append_setter`)
// that the generated Dart builder for a scalar list field (`Gadget.tags`,
// `tests/fixtures/builder_edge_cases.cstack`):
//
//   1. builds as `[]`, not a `StateError`, when the list was never touched;
//   2. gets a fluent `addTags(String)` append setter, beside the existing
//      bulk `tags(List<String>)` setter;
//   3. preserves append call order;
//   4. append allocates the list on first use, whether or not a prior bulk
//      `tags(...)` call had already allocated one;
//   5. the bulk setter still *replaces* — a bulk call after appends drops
//      everything appended before it, and vice versa an append after a
//      bulk call extends what the bulk call set rather than replacing it;
//   6. on `UpdateGadgetInput` (a Patch/"touched" input), appending marks
//      the field touched (produces a non-null list on the wire) while an
//      untouched list field stays `null` — the existing single-level
//      nullable representation every other Patch field already uses for
//      "the caller never touched this".
//   7. on `Gadget` itself (a ProjectionModel/model-class builder, not just
//      the Plain-kind Create/Update inputs above), an unset list field
//      also builds as `[]`, matching the Rust model builder's
//      `unwrap_or_default()` for the identical field.
//   8. `addTags` never mutates the list the caller passed to a prior
//      `.tags(...)` call in place — it must work whether that list came
//      from a growable literal, a `const` (unmodifiable) literal, or a
//      `fromWire`-decoded fixed-length list (every list this generator's
//      own `fromWire` produces is `.toList(growable: false)`).
//
// `just verify-dart` copies this into the `builder_edge_cases` fixture's
// generated `default`-preset package (library name `dart_verify_
// builder_edge_cases`) and runs it with `flutter test`.

import 'package:flutter_test/flutter_test.dart';

import 'package:dart_verify_builder_edge_cases/dart_verify_builder_edge_cases.dart';

CreateGadgetInputBuilder _baseCreateBuilder() {
  return CreateGadgetInputBuilder()
      .id(1)
      .builder('b')
      .newBuilder('nb')
      .setBuild('bd')
      .meta(null);
}

void main() {
  test('an unset list field builds as [], not a StateError', () {
    final gadget = _baseCreateBuilder().build();
    expect(gadget.tags, isEmpty);
    expect(gadget.tags, isA<List<String>>());
  });

  test('addTags appends in call order', () {
    final gadget = _baseCreateBuilder()
        .addTags('rust')
        .addTags('codegen')
        .addTags('dart')
        .build();
    expect(gadget.tags, ['rust', 'codegen', 'dart']);
  });

  test('addTags allocates the list on first use, without a prior bulk call', () {
    final gadget = _baseCreateBuilder().addTags('solo').build();
    expect(gadget.tags, ['solo']);
  });

  test('a bulk tags(...) call after appends replaces them entirely', () {
    final gadget = _baseCreateBuilder()
        .addTags('will-be-dropped')
        .tags(['replacement'])
        .build();
    expect(gadget.tags, ['replacement']);
  });

  test('addTags after a bulk tags(...) call extends it rather than replacing', () {
    final gadget = _baseCreateBuilder()
        .tags(['first'])
        .addTags('second')
        .build();
    expect(gadget.tags, ['first', 'second']);
  });

  test('the bulk setter alone still works and still replaces on repeat calls', () {
    final gadget = _baseCreateBuilder()
        .tags(['one', 'two'])
        .tags(['three'])
        .build();
    expect(gadget.tags, ['three']);
  });

  test('UpdateGadgetInput: a field nobody touched stays null, not []', () {
    final patch = UpdateGadgetInputBuilder().build();
    expect(patch.tags, isNull);
  });

  test('UpdateGadgetInput: addTags marks the field touched', () {
    final patch = UpdateGadgetInputBuilder().addTags('x').build();
    expect(patch.tags, ['x']);
  });

  test(
    'UpdateGadgetInput: append after a bulk call extends it, a bulk call '
    'after appends replaces them — same replace/extend contract as Create',
    () {
      final extended = UpdateGadgetInputBuilder()
          .tags(['a'])
          .addTags('b')
          .build();
      expect(extended.tags, ['a', 'b']);

      final replaced = UpdateGadgetInputBuilder()
          .addTags('dropped')
          .tags(['kept'])
          .build();
      expect(replaced.tags, ['kept']);
    },
  );

  test(
    'Gadget (model/ProjectionModel builder): an unset list field builds '
    'as [], not null',
    () {
      final gadget = GadgetBuilder()
          .id(1)
          .builder('b')
          .newBuilder('nb')
          .setBuild('bd')
          .meta(null)
          .build();
      expect(gadget.tags, isNotNull);
      expect(gadget.tags, isEmpty);
    },
  );

  test(
    'addTags appends onto a fixed-length list from fromWire without '
    'throwing',
    () {
      final decoded = Gadget.fromWire(<String, Object?>{
        'id': 1,
        'builder': 'b',
        'newBuilder': 'nb',
        'build': 'bd',
        'meta': null,
        'tags': ['from-wire'],
      });
      final gadget = _baseCreateBuilder()
          .tags(decoded.tags!)
          .addTags('codegen')
          .build();
      expect(gadget.tags, ['from-wire', 'codegen']);
    },
  );

  test('addTags appends onto an unmodifiable const list without throwing', () {
    final gadget = _baseCreateBuilder()
        .tags(const ['rust'])
        .addTags('appended')
        .build();
    expect(gadget.tags, ['rust', 'appended']);
  });
}
