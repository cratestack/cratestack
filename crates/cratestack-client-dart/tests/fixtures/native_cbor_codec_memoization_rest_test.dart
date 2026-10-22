// REST half of cratestack#798's memoization proof. See
// `native_cbor_codec_memoization_common.dart` for the contract itself and
// `tests/native_cbor_codec_memoization.rs` for how this gets into a
// generated package.
//
// Imports `src/runtime.dart` directly rather than the package's public
// entry point: the entry point re-exports `models.dart`, whose
// `@CratestackBuilder()` classes need a `build_runner` pass before
// anything can compile. The cached codec future lives in `runtime.dart`
// and nothing in this test touches a model, so that whole step is skipped.
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:native_cbor_memo_rest/src/runtime.dart';

import 'native_cbor_codec_memoization_common.dart';

/// Answers every request `204 No Content` without touching the network.
class _NoNetworkTransport implements CratestackHttpTransport {
  const _NoNetworkTransport();

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async =>
      CratestackHttpResponse.buffered(status: 204);
}

void main() {
  final adapter = CratestackHttpAdapter(
    const _NoNetworkTransport(),
    baseUri: Uri.parse('http://stub.invalid'),
  );

  runCodecMemoizationContract(
    'REST',
    () => adapter.execute(
      const CratestackRequest(
        method: 'POST',
        path: '/things',
        // Non-null: `_encodeBody` returns early for a null body and would
        // never reach the codec, making every assertion here vacuous.
        body: <String, Object?>{'name': 'thing'},
      ),
    ),
  );
  // After the contract above (which asserts a fresh isolate): the same
  // codec, exercised end to end. The stub's "CBOR" is UTF-8 JSON text, so a
  // transport that answers with the request body proves the adapter encodes
  // through `encodeJson` and decodes `application/cbor` through `decodeJson`.
  test('REST: the native path encodes and decodes through the codec', () async {
    final transport = _EchoTransport();
    final reply = await CratestackHttpAdapter(
      transport,
      baseUri: Uri.parse('http://stub.invalid'),
    ).execute(
      const CratestackRequest(
        method: 'POST',
        path: '/things',
        body: <String, Object?>{'name': 'thing', 'n': 1},
      ),
    );
    expect(utf8.decode(transport.lastBody!), '{"name":"thing","n":1}');
    expect(transport.lastHeaders!['Content-Type'], 'application/cbor');
    expect(reply, <String, Object?>{'name': 'thing', 'n': 1});
  });
}

/// Answers `200 application/cbor` with the request body it was given.
class _EchoTransport implements CratestackHttpTransport {
  Uint8List? lastBody;
  Map<String, String>? lastHeaders;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    lastBody = request.body;
    lastHeaders = request.headers;
    return CratestackHttpResponse.buffered(
      status: 200,
      headers: {'content-type': 'application/cbor'},
      body: request.body!,
    );
  }
}
