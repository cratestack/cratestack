// ADR 0019, PR B9, RPC half. Copied into a `default`-preset package generated
// from `bigint_scalar_rpc.cstack` (library `bigint_rpc_round_trip_check`) by
// `tests/bigint_round_trip.rs`, and run on the Dart VM, dart2js and dart2wasm
// like `bigint_round_trip_test.dart`.
//
// The REST client interpolates a primary key into the path (`/ledgers/$id`),
// which already prints a canonical decimal. The RPC client instead puts the
// key in a frame, `{'id': id}`, and a bare `BigInt` there would reach
// `jsonEncode` (which throws) or `package:cbor` (which writes an integer or a
// bignum, never the text string the server requires). So this records the
// frames the generated methods hand to a `CratestackRpcAdapter` and checks
// they hold strings, and that a reply decodes back to the exact value.
//
// No `dart:io`: the same file has to compile for the browser targets.

import 'dart:convert';

import 'package:bigint_rpc_round_trip_check/bigint_rpc_round_trip_check.dart';
import 'package:flutter_test/flutter_test.dart';

const i64Max = '9223372036854775807';
const i64Min = '-9223372036854775808';
const aboveSafe = '9007199254740993';

class _Call {
  const _Call(this.opId, this.input);

  final String opId;
  final Object? input;
}

class _RecordingAdapter implements CratestackRpcAdapter {
  final List<_Call> calls = <_Call>[];
  Object? reply;

  @override
  Future<Object?> call(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async {
    calls.add(_Call(opId, input));
    return reply;
  }

  @override
  Future<List<RpcResponseFrame>> batch(
    List<RpcRequest> requests, {
    CratestackRpcCallOptions? options,
  }) => throw UnimplementedError();

  @override
  Stream<Object?> stream(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) => throw UnimplementedError();
}

void main() {
  late _RecordingAdapter adapter;
  late BigintRpcRoundTripCheckCratestackClient client;

  setUp(() {
    adapter = _RecordingAdapter();
    client = BigintRpcRoundTripCheckCratestackClient(adapter);
  });

  group('a BigInt primary key is a string in the frame', () {
    for (final value in <String>[i64Max, i64Min, aboveSafe]) {
      test('get, update and delete send "$value", not a BigInt', () async {
        adapter.reply = <String, Object?>{'id': value, 'openingE8': value};

        final fetched = await client.ledgers.get(BigInt.parse(value));
        await client.ledgers.update(
          BigInt.parse(value),
          UpdateLedgerInput(openingE8: BigInt.parse(value)),
        );
        await client.ledgers.delete(BigInt.parse(value));

        expect(adapter.calls.map((call) => call.opId), <String>[
          'model.Ledger.get',
          'model.Ledger.update',
          'model.Ledger.delete',
        ]);
        expect(adapter.calls[0].input, <String, Object?>{'id': value});
        expect(adapter.calls[1].input, <String, Object?>{
          'id': value,
          'patch': <String, Object?>{'openingE8': value},
        });
        expect(adapter.calls[2].input, <String, Object?>{'id': value});
        for (final call in adapter.calls) {
          // The failure this guards: `jsonEncode` of a frame holding a BigInt.
          expect(jsonEncode(call.input), contains('"$value"'));
        }

        // The reply decodes to the exact value on this runtime.
        expect(fetched.id, BigInt.parse(value));
        expect(fetched.id.toString(), value);
        expect(fetched.openingE8, BigInt.parse(value));
      });
    }

    test('a create input and its reply carry strings both ways', () async {
      adapter.reply = <String, Object?>{'id': i64Max, 'openingE8': aboveSafe};
      final created = await client.ledgers.create(
        CreateLedgerInput(
          id: BigInt.parse(i64Max),
          openingE8: BigInt.parse(aboveSafe),
        ),
      );
      expect(adapter.calls.single.input, <String, Object?>{
        'id': i64Max,
        'openingE8': aboveSafe,
      });
      expect(created.id, BigInt.parse(i64Max));
      expect(created.openingE8, BigInt.parse(aboveSafe));
    });
  });

  group('procedures', () {
    test('a BigInt argument goes out as a string, a bare return decodes', () async {
      adapter.reply = i64Min;
      final result = await client.procedures.bump(
        BumpArgs(by: BigInt.parse(aboveSafe)),
      );
      expect(adapter.calls.single.opId, 'procedure.bump');
      expect(adapter.calls.single.input, <String, Object?>{'by': aboveSafe});
      expect(result, BigInt.parse(i64Min));
      expect(result.toString(), i64Min);
    });

    test('a bare BigInt return that is a number throws, naming the procedure', () async {
      adapter.reply = 9007199254740992;
      await expectLater(
        client.procedures.bump(BumpArgs(by: BigInt.one)),
        throwsA(
          isA<FormatException>().having(
            (error) => error.message,
            'message',
            allOf(contains('Procedure.bump'), contains('canonical decimal string')),
          ),
        ),
      );
    });

    test('a type reply with a BigInt list decodes exactly', () async {
      adapter.reply = <String, Object?>{
        'totalE8': i64Max,
        'perAccountE8': <Object?>[aboveSafe, i64Min],
        'maxE8': null,
      };
      final report = await client.procedures.report(
        ReportArgs(since: BigInt.parse(i64Min)),
      );
      expect(adapter.calls.single.input, <String, Object?>{'since': i64Min});
      expect(report.totalE8, BigInt.parse(i64Max));
      expect(report.perAccountE8, <BigInt>[
        BigInt.parse(aboveSafe),
        BigInt.parse(i64Min),
      ]);
      expect(report.maxE8, isNull);
    });
  });
}
