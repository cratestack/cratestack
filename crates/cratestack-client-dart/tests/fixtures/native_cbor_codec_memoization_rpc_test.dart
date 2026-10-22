// RPC half of cratestack#798's memoization proof — the same contract as
// the REST half, against the other transport's runtime. See
// `native_cbor_codec_memoization_common.dart`, and
// `native_cbor_codec_memoization_rest_test.dart`'s header for why this
// imports `src/runtime.dart` rather than the package entry point.
//
// Both transports are here because the cached-codec accessor is generated
// TWICE — `rest-runtime.dart.j2` and `rpc_runtime/types.dart.j2` each
// carry their own copy — so a fix applied to one proves nothing about the
// other.
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:native_cbor_memo_rpc/src/runtime.dart';

import 'native_cbor_codec_memoization_common.dart';

/// Answers every request `204 No Content` without touching the network.
class _NoNetworkTransport implements CratestackHttpTransport {
  const _NoNetworkTransport();

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async =>
      CratestackHttpResponse.buffered(status: 204);
}

void main() {
  final adapter = CratestackRpcHttpAdapter(
    const _NoNetworkTransport(),
    baseUri: Uri.parse('http://stub.invalid'),
  );

  runCodecMemoizationContract(
    'RPC',
    // Non-null input for the same reason the REST half passes a body:
    // `_encodeBody` short-circuits on null and never resolves the codec.
    () => adapter.call('procedure.echo', <String, Object?>{'name': 'thing'}),
  );
  // After the contract above (which asserts a fresh isolate): the same
  // codec, exercised end to end. See the REST half for why echoing works.
  test('RPC: the native path encodes and decodes through the codec', () async {
    final transport = _EchoTransport();
    final reply = await CratestackRpcHttpAdapter(
      transport,
      baseUri: Uri.parse('http://stub.invalid'),
    ).call('procedure.echo', const <String, Object?>{'name': 'thing', 'n': 1});
    expect(utf8.decode(transport.lastBody!), '{"name":"thing","n":1}');
    expect(reply, <String, Object?>{'name': 'thing', 'n': 1});
  });
}

/// Answers `200 application/cbor` with the request body it was given.
class _EchoTransport implements CratestackHttpTransport {
  Uint8List? lastBody;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    lastBody = request.body;
    return CratestackHttpResponse.buffered(
      status: 200,
      headers: {'content-type': 'application/cbor'},
      body: request.body!,
    );
  }
}
