// ADR 0019, PR B9: a `BigInt` through the generated REST client's real CBOR
// codec, against pinned bytes. Modelled on `native_cbor_echo_rest_test.dart`:
// a real `dart:io` `HttpServer`, a real `dio`, no fake adapter.
//
// `tests/bigint_round_trip.rs` copies this one file into two packages generated
// from `bigint_scalar.cstack` with the same library name
// (`bigint_native_cbor_echo_rest_verify`) and runs it with `flutter test` in
// each, so the identical expectations hold for both codec choices:
//
//   * `native_cbor: true`   `cratestack_cbor`, flutter_rust_bridge over a JSON
//                           text boundary (`jsonEncode` then Rust, and back),
//   * `native_cbor: false`  pure-Dart `package:cbor` (`src/config.rs`).
//
// What goes on the wire is `{"amountE8": "<decimal>"}`: a CBOR map of one pair
// whose value is a text string (major type 3), never an integer (major 0/1) and
// never a bignum (tag 2/3). The hex below is RFC 8949 worked by hand and is the
// same table `cratestack-cbor-napi`, `cratestack-cbor-wasm`,
// `cratestack-client-flutter` and `cratestack_cbor`'s `shared_fixtures.dart`
// pin, so a byte that drifts in any language fails somewhere. The server side
// here deliberately speaks plain `package:cbor`, so the client's codec is
// judged against an independent decoder, not against itself.
//
// VM only (`dart:io`); the browser targets are covered by
// `bigint_round_trip_test.dart` and, for the web codec, `bigint_web_cbor_test.dart`.

@TestOn('vm')
library;

import 'dart:io';
import 'dart:typed_data';

import 'package:bigint_native_cbor_echo_rest_verify/bigint_native_cbor_echo_rest_verify.dart';
import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

class _Fixture {
  const _Fixture(this.decimal, this.hex);

  final String decimal;
  final String hex;
}

const fixtures = <_Fixture>[
  _Fixture(
    '9223372036854775807',
    'a168616d6f756e7445387339323233333732303336383534373735383037',
  ),
  _Fixture(
    '-9223372036854775808',
    'a168616d6f756e744538742d39323233333732303336383534373735383038',
  ),
  _Fixture(
    '9007199254740993',
    'a168616d6f756e7445387039303037313939323534373430393933',
  ),
  _Fixture('0', 'a168616d6f756e7445386130'),
  _Fixture('-1', 'a168616d6f756e744538622d31'),
];

String _hex(List<int> bytes) =>
    bytes.map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();

Uint8List _unhex(String hex) => Uint8List.fromList(<int>[
  for (var i = 0; i < hex.length; i += 2)
    int.parse(hex.substring(i, i + 2), radix: 16),
]);

void main() {
  late HttpServer server;
  late BigintNativeCborEchoRestVerifyCratestackClient client;
  final requestBodies = <Uint8List>[];
  late Uint8List replyBytes;

  setUp(() async {
    requestBodies.clear();
    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((request) async {
      final bytes = <int>[];
      await for (final chunk in request) {
        bytes.addAll(chunk);
      }
      requestBodies.add(Uint8List.fromList(bytes));
      request.response.statusCode = 200;
      request.response.headers.set('content-type', 'application/cbor');
      request.response.add(replyBytes);
      await request.response.close();
    });
    client = BigintNativeCborEchoRestVerifyCratestackClient(
      CratestackCborDioAdapter(
        dio: Dio(BaseOptions(baseUrl: 'http://127.0.0.1:${server.port}')),
      ),
      basePath: '',
    );
  });

  tearDown(() => server.close(force: true));

  for (final fixture in fixtures) {
    test('${fixture.decimal} encodes to the pinned bytes and decodes exactly', () async {
      replyBytes = _unhex(fixture.hex);

      final reply = await client.procedures.echoAmount(
        EchoAmountArgs(amountE8: BigInt.parse(fixture.decimal)),
      );

      // Request: what the client's codec wrote.
      expect(requestBodies, hasLength(1));
      expect(_hex(requestBodies.single), fixture.hex);
      // The value's header byte sits after `a1`, `68` and the 8-byte key:
      // major type 3 (text string), never 0 or 1 (integer) nor a tag.
      expect(requestBodies.single[10] >> 5, 3);
      // Judged by an independent decoder too, not only by the bytes.
      final decoded = cratestackNormalizeWire(
        cbor.cbor.decode(requestBodies.single),
      );
      expect(decoded, <String, Object?>{'amountE8': fixture.decimal});

      // Response: the pinned bytes, decoded by the client's codec.
      expect(reply.amountE8, BigInt.parse(fixture.decimal));
      expect(reply.amountE8.toString(), fixture.decimal);
    });
  }

  test('a CBOR integer where a BigInt string belongs is refused', () async {
    // `{"amountE8": 1}`: a pre-cutover server's number, major type 0.
    replyBytes = _unhex('a168616d6f756e74453801');
    await expectLater(
      client.procedures.echoAmount(EchoAmountArgs(amountE8: BigInt.one)),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          allOf(
            contains('Amount.amountE8'),
            contains('canonical decimal string'),
          ),
        ),
      ),
    );
  });

  test('a CBOR bignum (tag 2) where a BigInt string belongs is refused', () async {
    // `{"amountE8": 2(h'01')}`: tag 2 over a one-byte string. Both codecs hand
    // the decoder a number or a BigInt here, never the String it requires.
    replyBytes = _unhex('a168616d6f756e744538c24101');
    await expectLater(
      client.procedures.echoAmount(EchoAmountArgs(amountE8: BigInt.one)),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Amount.amountE8'),
        ),
      ),
    );
  });
}
