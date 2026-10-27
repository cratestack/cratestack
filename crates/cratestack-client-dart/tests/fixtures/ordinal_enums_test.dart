// Ordinal enums (`Pending = 1`, docs/design/enum-ordinals.md): a running
// proof, beside `tests/enum_ordinals.rs`'s text-level assertions, that a
// generated ordinal enum is sent as its number, decodes the number or the
// name (a peer still sending names keeps working), and refuses anything
// else, in a model, a `type` and a list.
//
// `just verify-dart` copies this into the `ordinal_enums` fixture's
// generated `default`-preset package (library name `dart_verify_
// ordinal_enums`) and runs it with `flutter test`.

import 'package:flutter_test/flutter_test.dart';

import 'package:dart_verify_ordinal_enums/dart_verify_ordinal_enums.dart';

void main() {
  test('a variant carries its ordinal and its schema name', () {
    expect(PaymentStatus.refunded.wireValue, 5);
    expect(PaymentStatus.refunded.wireName, 'Refunded');
    expect(PaymentStatus.approved.toWire(), 2);
  });

  test('fromWire takes the number or the name', () {
    expect(PaymentStatus.fromWire(2), PaymentStatus.approved);
    expect(PaymentStatus.fromWire('Approved'), PaymentStatus.approved);
  });

  test('a reserved number, an unknown name or another type is refused', () {
    for (final bad in <Object?>[3, 'Settled', 'approved', 2.5, null]) {
      expect(() => PaymentStatus.fromWire(bad), throwsArgumentError, reason: '$bad');
    }
  });

  test('a model round-trips its ordinal fields as numbers', () {
    final payment = Payment.fromWire(<String, Object?>{
      'id': 1,
      'status': 2,
      'previous': 'Refunded',
      'channel': 'card',
    });
    expect(payment.status, PaymentStatus.approved);
    expect(payment.previous, PaymentStatus.refunded);
    final wire = payment.toWire();
    expect(wire['status'], 2);
    expect(wire['previous'], 5);
    // A string enum beside it is untouched.
    expect(wire['channel'], 'card');
  });

  test('a type with an ordinal list round-trips', () {
    final summary = Summary.fromWire(<String, Object?>{
      'current': 1,
      'history': <Object?>[1, 'Approved', 5],
    });
    expect(summary.history, <PaymentStatus>[
      PaymentStatus.pending,
      PaymentStatus.approved,
      PaymentStatus.refunded,
    ]);
    expect(summary.toWire()['history'], <int>[1, 2, 5]);
  });
}
