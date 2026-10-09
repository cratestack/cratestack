// ADR 0019, PR B9, riverpod preset only. `bigint_round_trip_test.dart` already
// runs against this preset's package (same constructors, same `fromWire` and
// `toWire`, same REST client); this file covers what only this preset adds:
//
//   * every data class is a `@MappableClass(generateMethods: equals | copy)`,
//     so `==` and `hashCode` come from dart_mappable, which has no `BigInt`
//     mapper and falls back to `BigInt`'s own value equality. If that were
//     ever not so, `AccountWhere` would compare by identity and a riverpod
//     family keyed on a filter would never dedupe;
//   * a `@riverpod` provider takes the `BigInt` primary key as its family
//     argument, and the family cache dedupes by `==` and `hashCode`.
//
// 2^53 and 2^53 + 1 are the pair that a double-backed key would conflate.
//
// Copied into the package generated from `bigint_scalar.cstack` (library
// `bigint_round_trip_check`, riverpod preset). No `dart:io`, so it runs on the
// VM, dart2js and dart2wasm.

import 'package:bigint_round_trip_check/bigint_round_trip_check.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

const i64Max = '9223372036854775807';
const i64Min = '-9223372036854775808';
const aboveSafe = '9007199254740993';
const safe = '9007199254740992';

class _RecordingAdapter implements CratestackClientAdapter {
  final List<CratestackRequest> requests = <CratestackRequest>[];

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    requests.add(request);
    final id = request.path.split('/').last;
    return <String, Object?>{'id': id, 'openingE8': id};
  }
}

BigInt _big(String text) => BigInt.parse(text);

void main() {
  group('generated == and hashCode over BigInt fields', () {
    test('equal-valued filters built separately are equal and hash alike', () {
      final first = BigIntFilter(
        eq: _big(i64Max),
        in$: <BigInt>[_big(aboveSafe), _big(i64Min)],
      );
      final second = BigIntFilter(
        eq: _big(i64Max),
        in$: <BigInt>[_big(aboveSafe), _big(i64Min)],
      );
      expect(identical(first, second), isFalse);
      expect(first, equals(second));
      expect(first.hashCode, second.hashCode);
    });

    test('2^53 and 2^53 + 1 are different filters', () {
      expect(
        BigIntFilter(eq: _big(aboveSafe)),
        isNot(equals(BigIntFilter(eq: _big(safe)))),
      );
      expect(
        BigIntFilter(in$: <BigInt>[_big(aboveSafe)]),
        isNot(equals(BigIntFilter(in$: <BigInt>[_big(safe)]))),
      );
    });

    test('a per-model Where compares through its BigIntFilter fields', () {
      final first = AccountWhere(
        balanceE8: BigIntFilter(gte: _big(i64Min)),
        ledgerId: BigIntFilter(eq: _big(aboveSafe)),
      );
      final second = AccountWhere(
        balanceE8: BigIntFilter(gte: _big(i64Min)),
        ledgerId: BigIntFilter(eq: _big(aboveSafe)),
      );
      expect(first, equals(second));
      expect(first.hashCode, second.hashCode);
      expect(
        first,
        isNot(
          equals(
            AccountWhere(
              balanceE8: BigIntFilter(gte: _big(i64Min)),
              ledgerId: BigIntFilter(eq: _big(safe)),
            ),
          ),
        ),
      );
    });

    test('copyWith keeps the exact value and changes only what it is given', () {
      final where = AccountWhere(balanceE8: BigIntFilter(eq: _big(i64Max)));
      final copied = where.copyWith(
        ledgerId: BigIntFilter(eq: _big(aboveSafe)),
      );
      expect(copied.balanceE8!.eq, _big(i64Max));
      expect(copied.ledgerId!.eq, _big(aboveSafe));
      expect(where.ledgerId, isNull);
    });

    test('models and types carrying BigInts compare by value', () {
      expect(
        Account(balanceE8: _big(i64Max), ledgerId: _big(aboveSafe)),
        equals(Account(balanceE8: _big(i64Max), ledgerId: _big(aboveSafe))),
      );
      expect(
        Account(balanceE8: _big(aboveSafe)),
        isNot(equals(Account(balanceE8: _big(safe)))),
      );
      expect(
        Report(
          totalE8: _big(i64Min),
          perAccountE8: <BigInt>[_big(i64Max), _big(aboveSafe)],
        ),
        equals(
          Report(
            totalE8: _big(i64Min),
            perAccountE8: <BigInt>[_big(i64Max), _big(aboveSafe)],
          ),
        ),
      );
    });

    test('a Where survives toWire and fromWire with its values intact', () {
      final where = AccountWhere(
        balanceE8: BigIntFilter(
          gt: _big(aboveSafe),
          lt: _big(i64Max),
          in$: <BigInt>[_big(i64Min)],
        ),
      );
      expect(AccountWhere.fromWire(where.toWire()), equals(where));
    });
  });

  group('a BigInt primary key as a riverpod family argument', () {
    test('equal keys are one provider, 2^53 and 2^53 + 1 are two', () {
      expect(ledgerProvider(_big(i64Max)), ledgerProvider(_big(i64Max)));
      expect(
        ledgerProvider(_big(i64Min)).hashCode,
        ledgerProvider(_big(i64Min)).hashCode,
      );
      expect(
        ledgerProvider(_big(aboveSafe)),
        isNot(equals(ledgerProvider(_big(safe)))),
      );
    });

    test('a read fetches the decimal path once per distinct key', () async {
      final adapter = _RecordingAdapter();
      final container = ProviderContainer(
        overrides: [
          bigintRoundTripCheckAdapterProvider.overrideWithValue(adapter),
        ],
      );
      addTearDown(container.dispose);

      // `@riverpod` providers are auto-dispose: hold a listener per read, or
      // the cache entry is dropped between reads and nothing is deduped.
      Future<Ledger> read(String key) {
        final provider = ledgerProvider(_big(key));
        final subscription = container.listen(provider, (previous, next) {});
        addTearDown(subscription.close);
        return container.read(provider.future);
      }

      final first = await read(i64Max);
      // A separately built, equal-valued key reuses the cached value.
      final again = await read(i64Max);
      final other = await read(aboveSafe);

      expect(first.id, _big(i64Max));
      expect(again.id, _big(i64Max));
      expect(other.id, _big(aboveSafe));
      expect(adapter.requests.map((request) => request.path), <String>[
        '/api/ledgers/$i64Max',
        '/api/ledgers/$aboveSafe',
      ]);
    });
  });
}
