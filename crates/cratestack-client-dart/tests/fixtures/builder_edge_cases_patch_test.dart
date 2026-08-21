// cratestack#663: real, running proof (not just the text-level assertions
// in `tests/generator.rs`) that the generated `UpdateGadgetInput`/
// `UpdateGadgetInputBuilder` (`Gadget.note`, a nullable/`Optional`-arity
// column — `tests/fixtures/builder_edge_cases.cstack`) put exactly one
// wire representation on an untouched field, matching the generated Rust
// client's `Option<Option<T>>` for the same column, and keep the
// explicit-clear state (cratestack#567) reachable:
//
//   1. an untouched `Required`-arity field (`builder`) is absent from
//      `toWire()`, not sent as `null`.
//   2. an untouched nullable field (`note`) is ALSO absent from
//      `toWire()` — before this fix every generated field, including
//      nullable ones, was sent unconditionally, `note` included.
//   3. a nullable field explicitly cleared (`.note(null)`) still
//      serializes as an explicit wire `null`, distinguishable from
//      "untouched" via the key's presence, not its value.
//   4. a nullable field explicitly set to a real value serializes as
//      that value.
//   5. `UpdateGadgetInput.fromWire` round-trips all three states back
//      through `noteIsSet`/`note`, so a decoded "untouched" input
//      re-encodes to the same absent-key wire shape (not a `null`).
//   6. a nullable field set to a real value via DIRECT construction
//      (`UpdateGadgetInput(note: 'x')`, not the builder) still puts it
//      on the wire, even though `noteIsSet` defaults to `false` — the
//      constructor is public and plain, so this is not a builder-only
//      guarantee (cratestack#663 review round: `noteIsSet` alone was an
//      insufficient wire-write guard and silently dropped this case).
//
// `just verify-dart` copies this into the `builder_edge_cases` fixture's
// generated `default`-preset package (library name `dart_verify_
// builder_edge_cases`) and runs it with `flutter test`, beside
// `builder_edge_cases_list_test.dart`.

import 'package:flutter_test/flutter_test.dart';

import 'package:dart_verify_builder_edge_cases/dart_verify_builder_edge_cases.dart';

void main() {
  test('an untouched Required-arity field is absent from the wire', () {
    final patch = UpdateGadgetInputBuilder().note('set').build();
    final wire = patch.toWire();
    expect(wire.containsKey('builder'), isFalse);
  });

  test('an untouched nullable field is absent from the wire, not sent as null', () {
    final patch = UpdateGadgetInputBuilder().builder('renamed').build();
    final wire = patch.toWire();
    expect(wire.containsKey('note'), isFalse);
    expect(wire.containsKey('builder'), isTrue);
    expect(wire['builder'], 'renamed');
  });

  test('an explicitly-cleared nullable field serializes as an explicit null', () {
    final patch = UpdateGadgetInputBuilder().note(null).build();
    final wire = patch.toWire();
    expect(wire.containsKey('note'), isTrue);
    expect(wire['note'], isNull);
  });

  test('a nullable field explicitly set to a value serializes as that value', () {
    final patch = UpdateGadgetInputBuilder().note('hello').build();
    final wire = patch.toWire();
    expect(wire.containsKey('note'), isTrue);
    expect(wire['note'], 'hello');
  });

  test('calling the direct constructor without touching note leaves it untouched', () {
    const patch = UpdateGadgetInput(builder: 'renamed');
    expect(patch.noteIsSet, isFalse);
    expect(patch.toWire().containsKey('note'), isFalse);
  });

  test(
    'calling the direct constructor WITH a non-null note value still puts it on the wire, '
    'even though noteIsSet defaults to false (cratestack#663 review fix — the plain public '
    'const constructor is a legitimate way to build this class, not just the builder, and a '
    'caller-supplied value must never be silently dropped)',
    () {
      const patch = UpdateGadgetInput(builder: 'renamed', note: 'set directly');
      // The bug this test guards: before the fix, `toWire()` guarded on
      // `noteIsSet` alone, which only the builder or `fromWire` ever set —
      // a direct-construction caller who passed a real value here had it
      // silently dropped, with no error.
      expect(patch.noteIsSet, isFalse);
      final wire = patch.toWire();
      expect(wire.containsKey('note'), isTrue);
      expect(wire['note'], 'set directly');
    },
  );

  test('fromWire round-trips "untouched" back to an absent key', () {
    final decoded = UpdateGadgetInput.fromWire(<String, Object?>{'builder': 'renamed'});
    expect(decoded.noteIsSet, isFalse);
    expect(decoded.toWire().containsKey('note'), isFalse);
  });

  test('fromWire round-trips an explicit clear back to an explicit null', () {
    final decoded = UpdateGadgetInput.fromWire(<String, Object?>{'note': null});
    expect(decoded.noteIsSet, isTrue);
    expect(decoded.note, isNull);
    final wire = decoded.toWire();
    expect(wire.containsKey('note'), isTrue);
    expect(wire['note'], isNull);
  });

  test('fromWire round-trips an explicit value back to that value', () {
    final decoded = UpdateGadgetInput.fromWire(<String, Object?>{'note': 'from wire'});
    expect(decoded.noteIsSet, isTrue);
    expect(decoded.note, 'from wire');
    expect(decoded.toWire()['note'], 'from wire');
  });
}
