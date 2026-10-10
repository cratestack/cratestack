// ADR 0019, PR B9: the generated Dart client's `BigInt` support, as behaviour
// rather than as text. Copied into a package generated from
// `bigint_scalar.cstack` (library `bigint_round_trip_check`, `default` preset
// and `riverpod` preset alike: the constructors, `fromWire`/`toWire` and the
// REST client are the same API in both) by `tests/bigint_round_trip.rs`, which
// runs it three times:
//
//   * `flutter test`                          the Dart VM,
//   * `flutter test --platform chrome`        dart2js,
//   * `flutter test --platform chrome --wasm` dart2wasm.
//
// The point of running it three times is that one generated type has to be
// right on every target. On dart2js an `int` is a JS double, so a `jsonDecode`
// or `int.parse` of `9007199254740993` yields `9007199254740992`; the first
// group below is the one a decode through `int` fails on that runtime only.
//
// No `dart:io` here, so the same file compiles for the browser targets. No
// integer literal above 2^53 either: dart2js would round it at compile time,
// so every boundary value is a string.

import 'dart:convert';

import 'package:bigint_round_trip_check/bigint_round_trip_check.dart';
import 'package:flutter_test/flutter_test.dart';

const i64Max = '9223372036854775807';
const i64Min = '-9223372036854775808';
// 2^53 + 1: the smallest positive integer a double cannot hold.
const aboveSafe = '9007199254740993';
const boundary = <String>[i64Max, i64Min, aboveSafe];

class _RecordingAdapter implements CratestackClientAdapter {
  final List<CratestackRequest> requests = <CratestackRequest>[];
  Object? reply;

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    requests.add(request);
    return reply;
  }
}

Matcher _formatExceptionNaming(String site) => throwsA(
  isA<FormatException>().having(
    (error) => error.message,
    'message',
    allOf(contains(site), contains('canonical decimal string')),
  ),
);

void main() {
  group('exactness on this runtime', () {
    for (final value in boundary) {
      test('$value survives fromWire and toWire unchanged', () {
        final account = Account.fromWire(<String, Object?>{
          'id': 'acc_1',
          'holder': 'Ada',
          'balanceE8': value,
          'limitE8': null,
          'ledgerId': value,
        });
        expect(account.balanceE8, BigInt.parse(value));
        expect(account.balanceE8.toString(), value);
        expect(account.limitE8, isNull);
        expect(account.ledgerId.toString(), value);

        final wire = account.toWire();
        expect(wire['balanceE8'], isA<String>());
        expect(wire['balanceE8'], value);
        expect(wire['limitE8'], isNull);
        expect(Account.fromWire(wire).balanceE8, account.balanceE8);
      });
    }

    test('2^53 + 1 is not 2^53, which is what an int decode gets wrong', () {
      final decoded = Account.fromWire(<String, Object?>{
        'balanceE8': aboveSafe,
      }).balanceE8!;
      expect(decoded, isNot(BigInt.parse('9007199254740992')));
      expect(decoded - BigInt.parse('9007199254740992'), BigInt.one);
      expect(decoded + BigInt.one, BigInt.parse('9007199254740994'));
    });

    test('the i64 extremes are exactly the bounds, and compare as numbers', () {
      final max = BigInt.parse(i64Max);
      final min = BigInt.parse(i64Min);
      expect(max - BigInt.one, BigInt.parse('9223372036854775806'));
      expect(min + BigInt.one, BigInt.parse('-9223372036854775807'));
      expect(max.bitLength, 63);
      expect(min.bitLength, 63);
      expect(min < max, isTrue);
    });
  });

  group('every place a BigInt can sit', () {
    test('a required input field and its missing-key error', () {
      final input = CreateAccountInput.fromWire(<String, Object?>{
        'holder': 'Ada',
        'balanceE8': i64Min,
        'limitE8': aboveSafe,
        'ledgerId': i64Max,
      });
      expect(input.balanceE8, BigInt.parse(i64Min));
      expect(input.limitE8, BigInt.parse(aboveSafe));
      expect(input.ledgerId, BigInt.parse(i64Max));
      expect(input.toWire(), <String, Object?>{
        'holder': 'Ada',
        'balanceE8': i64Min,
        'limitE8': aboveSafe,
        'ledgerId': i64Max,
      });
      expect(
        () => CreateAccountInput.fromWire(<String, Object?>{
          'holder': 'Ada',
          'ledgerId': i64Max,
        }),
        throwsA(
          isA<FormatException>().having(
            (error) => error.message,
            'message',
            contains('CreateAccountInput.balanceE8'),
          ),
        ),
      );
    });

    test('an optional field accepts null and omits nothing it should keep', () {
      final input = CreateAccountInput.fromWire(<String, Object?>{
        'holder': 'Ada',
        'balanceE8': '0',
        'limitE8': null,
        'ledgerId': '-1',
      });
      expect(input.limitE8, isNull);
      expect(input.balanceE8, BigInt.zero);
      expect(input.ledgerId, BigInt.from(-1));
      expect(input.toWire()['limitE8'], isNull);
    });

    test('a BigInt primary key and a BigInt inside a relation', () {
      final account = Account.fromWire(<String, Object?>{
        'balanceE8': i64Max,
        'ledgerId': aboveSafe,
        'ledger': <String, Object?>{'id': aboveSafe, 'openingE8': i64Min},
      });
      expect(account.ledger, isNotNull);
      expect(account.ledger!.id, BigInt.parse(aboveSafe));
      expect(account.ledger!.openingE8, BigInt.parse(i64Min));
      expect(account.toWire()['ledger'], <String, Object?>{
        'id': aboveSafe,
        'openingE8': i64Min,
      });
    });

    test('a type with a BigInt list and an optional BigInt', () {
      final report = Report.fromWire(<String, Object?>{
        'totalE8': i64Min,
        'perAccountE8': <Object?>[i64Max, aboveSafe, '0'],
        'maxE8': null,
      });
      expect(report.totalE8, BigInt.parse(i64Min));
      expect(report.perAccountE8, <BigInt>[
        BigInt.parse(i64Max),
        BigInt.parse(aboveSafe),
        BigInt.zero,
      ]);
      expect(report.maxE8, isNull);
      expect(report.toWire(), <String, Object?>{
        'totalE8': i64Min,
        'perAccountE8': <String>[i64Max, aboveSafe, '0'],
        'maxE8': null,
      });
    });

    test('procedure arguments encode to strings, not bare BigInts', () {
      expect(EchoAmountArgs(amountE8: BigInt.parse(i64Min)).toWire(), {
        'amountE8': i64Min,
      });
      expect(BumpArgs(by: BigInt.parse(aboveSafe)).toWire(), {
        'by': aboveSafe,
      });
      // `jsonEncode` throws on a bare BigInt; it must not get one.
      expect(
        jsonEncode(ReportArgs(since: BigInt.parse(i64Max)).toWire()),
        '{"since":"$i64Max"}',
      );
      final reply = Amount.fromWire(<String, Object?>{'amountE8': aboveSafe});
      expect(reply.amountE8, BigInt.parse(aboveSafe));
    });

    test('a BigInt inside @computed params is wire-equal and hashes alike', () {
      final first = AccountComputedParams(
        summary: BalanceParams(floorE8: BigInt.parse(i64Max)),
      );
      final second = AccountComputedParams(
        summary: BalanceParams(floorE8: BigInt.parse(i64Max)),
      );
      expect(jsonEncode(first.toWire()), '{"summary":{"floorE8":"$i64Max"}}');
      expect(first, equals(second));
      expect(first.hashCode, second.hashCode);
      expect(
        first,
        isNot(
          equals(
            AccountComputedParams(
              summary: BalanceParams(floorE8: BigInt.parse(aboveSafe)),
            ),
          ),
        ),
      );
    });
  });

  group('the REST client', () {
    late _RecordingAdapter adapter;
    late BigintRoundTripCheckCratestackClient client;

    setUp(() {
      adapter = _RecordingAdapter();
      client = BigintRoundTripCheckCratestackClient(adapter);
    });

    for (final value in boundary) {
      test('a BigInt key $value is the decimal text in the path', () async {
        adapter.reply = <String, Object?>{'id': value, 'openingE8': value};

        final fetched = await client.ledgers.get(BigInt.parse(value));
        await client.ledgers.update(
          BigInt.parse(value),
          UpdateLedgerInput(openingE8: BigInt.parse(value)),
        );
        await client.ledgers.delete(BigInt.parse(value));

        expect(adapter.requests.map((request) => request.method), <String>[
          'GET',
          'PATCH',
          'DELETE',
        ]);
        for (final request in adapter.requests) {
          expect(request.path, '/api/ledgers/$value');
        }
        expect(adapter.requests[1].body, <String, Object?>{
          'openingE8': value,
        });
        expect(fetched.id, BigInt.parse(value));
        expect(fetched.openingE8, BigInt.parse(value));
      });
    }

    test('create and a procedure send strings, not bare BigInts', () async {
      adapter.reply = <String, Object?>{'id': i64Max, 'openingE8': aboveSafe};
      await client.ledgers.create(
        CreateLedgerInput(
          id: BigInt.parse(i64Max),
          openingE8: BigInt.parse(aboveSafe),
        ),
      );
      expect(adapter.requests.single.body, <String, Object?>{
        'id': i64Max,
        'openingE8': aboveSafe,
      });
      expect(jsonEncode(adapter.requests.single.body), contains('"$i64Max"'));

      adapter.reply = i64Min;
      final bumped = await client.procedures.bump(
        BumpArgs(by: BigInt.parse(aboveSafe)),
      );
      expect(adapter.requests.last.body, <String, Object?>{'by': aboveSafe});
      expect(bumped, BigInt.parse(i64Min));
    });
  });

  group('BigIntFilter and the per-model Where', () {
    test('decodes every operand to a BigInt and re-encodes the same text', () {
      final filter = BigIntFilter.fromWire(<String, Object?>{
        'eq': i64Max,
        'ne': i64Min,
        'in': <Object?>[i64Max, i64Min, aboveSafe],
        'lt': aboveSafe,
        'lte': aboveSafe,
        'gt': i64Min,
        'gte': '0',
        'isNull': false,
      });
      expect(filter.eq, BigInt.parse(i64Max));
      expect(filter.ne, BigInt.parse(i64Min));
      expect(filter.in$, hasLength(3));
      expect(filter.in$![2], BigInt.parse(aboveSafe));
      expect(filter.isNull, isFalse);
      expect(filter.toWire(), <String, Object?>{
        'eq': i64Max,
        'ne': i64Min,
        'in': <String>[i64Max, i64Min, aboveSafe],
        'lt': aboveSafe,
        'lte': aboveSafe,
        'gt': i64Min,
        'gte': '0',
        'isNull': false,
      });
    });

    test('a filter built by hand puts strings on the wire', () {
      final where = AccountWhere(
        balanceE8: BigIntFilter(
          gt: BigInt.parse(aboveSafe),
          in$: <BigInt>[BigInt.parse(i64Max), BigInt.parse(i64Min)],
        ),
      );
      final wire = where.toWire();
      expect(jsonEncode(wire['balanceE8']), contains('"gt":"$aboveSafe"'));
      expect(
        AccountWhere.fromWire(wire).balanceE8!.gt,
        BigInt.parse(aboveSafe),
      );
    });

    test('the primary key of a BigInt-keyed model is filterable', () {
      final where = LedgerWhere.fromWire(<String, Object?>{
        'id': <String, Object?>{'eq': i64Max},
      });
      expect(where.id!.eq, BigInt.parse(i64Max));
    });

    test('a BigInt field is a BigIntFilter, not a NumberFilter', () {
      // Compile-time: `NumberFilter?` here would not analyze. Runtime: the
      // class the field carries is the BigInt one.
      final where = AccountWhere(balanceE8: const BigIntFilter());
      expect(where.balanceE8, isA<BigIntFilter>());
      expect(where.balanceE8, isNot(isA<NumberFilter>()));
    });
  });

  group('a number or a non-canonical string at a BigInt key throws', () {
    // The numbers a pre-cutover server would send. None is written above 2^53
    // (dart2js would round the literal itself); the rounded 2^53 stands in for
    // the value that "already lost a digit" and must not be accepted.
    final numbers = <Object?>[1, 0, -5, 9007199254740992, 1.5, true];

    test('in a model field', () {
      for (final number in numbers) {
        expect(
          () => Account.fromWire(<String, Object?>{'balanceE8': number}),
          _formatExceptionNaming('Account.balanceE8'),
          reason: 'a ${number.runtimeType} must not decode as a BigInt',
        );
      }
    });

    test('in a required field, an optional field and a primary key', () {
      expect(
        () => CreateAccountInput.fromWire(<String, Object?>{
          'holder': 'Ada',
          'balanceE8': 12,
          'ledgerId': '1',
        }),
        _formatExceptionNaming('CreateAccountInput.balanceE8'),
      );
      expect(
        () => CreateAccountInput.fromWire(<String, Object?>{
          'holder': 'Ada',
          'balanceE8': '1',
          'limitE8': 12,
          'ledgerId': '1',
        }),
        _formatExceptionNaming('CreateAccountInput.limitE8'),
      );
      expect(
        () => Ledger.fromWire(<String, Object?>{'id': 7}),
        _formatExceptionNaming('Ledger.id'),
      );
    });

    test('in a list item, a nested relation and a procedure return', () {
      expect(
        () => Report.fromWire(<String, Object?>{
          'totalE8': '1',
          'perAccountE8': <Object?>['1', 2],
        }),
        _formatExceptionNaming('Report.perAccountE8'),
      );
      expect(
        () => Account.fromWire(<String, Object?>{
          'ledger': <String, Object?>{'id': 3},
        }),
        _formatExceptionNaming('Ledger.id'),
      );
      expect(
        () => Amount.fromWire(<String, Object?>{'amountE8': 9007199254740992}),
        _formatExceptionNaming('Amount.amountE8'),
      );
    });

    test('in a BigIntFilter operand and its in list', () {
      expect(
        () => BigIntFilter.fromWire(<String, Object?>{'eq': 5}),
        _formatExceptionNaming('BigIntFilter.eq'),
      );
      expect(
        () => BigIntFilter.fromWire(<String, Object?>{
          'in': <Object?>['1', 2],
        }),
        _formatExceptionNaming('BigIntFilter.in'),
      );
    });

    test('a string that is not the canonical form is refused', () {
      // `BigInt.parse` itself would take several of these (`+5`, `007`, ` 1`)
      // and turn `0x1F` into 31; the generated decode does not.
      for (final text in <String>[
        '',
        '+5',
        '007',
        '-0',
        '-',
        '--1',
        ' 1',
        '1 ',
        '1\n',
        '1.0',
        '1e3',
        '0x1F',
        '1_000',
        '٣', // an Arabic-Indic digit three
      ]) {
        expect(
          () => Account.fromWire(<String, Object?>{'balanceE8': text}),
          _formatExceptionNaming('Account.balanceE8'),
          reason: '"$text" is not canonical',
        );
      }
    });

    test('canonical strings are accepted, whatever their size', () {
      for (final text in <String>['0', '-1', '10', '-10', i64Max, i64Min]) {
        expect(
          Account.fromWire(<String, Object?>{'balanceE8': text}).balanceE8,
          BigInt.parse(text),
        );
      }
      // The i64 bound belongs to the server (it answers 422); a client holds
      // an arbitrary-precision BigInt and does not second-guess the range.
      const beyond = '9223372036854775808';
      expect(
        Account.fromWire(<String, Object?>{'balanceE8': beyond}).balanceE8,
        BigInt.parse(beyond),
      );
    });
  });
}
