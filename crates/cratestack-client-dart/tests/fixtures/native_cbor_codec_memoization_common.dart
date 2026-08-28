// Shared driver for the REST and RPC halves of cratestack#798's
// memoization proof — copied into a generated package's `test/` directory
// by `tests/native_cbor_codec_memoization.rs` alongside one of the two
// `*_test.dart` files beside it.
//
// Shared, not duplicated per transport, on purpose: the contract is
// supposed to be *identical* on both, and the cheapest way to keep it that
// way is to let one set of assertions run against both. If the two
// runtimes ever drift, this file stops compiling or starts failing for one
// of them — which is the signal we want.
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

/// A `Dio` whose transport never leaves the process — every request
/// answers `204 No Content`, which the generated adapters short-circuit
/// before decoding. The request half is what matters here: it is the
/// encode path that resolves the codec.
Dio stubDio() {
  final dio = Dio(BaseOptions(baseUrl: 'http://stub.invalid'));
  dio.httpClientAdapter = _NoNetworkAdapter();
  return dio;
}

class _NoNetworkAdapter implements HttpClientAdapter {
  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async => ResponseBody.fromBytes(const <int>[], 204);

  @override
  void close({bool force = false}) {}
}

/// Runs the whole contract against [issueRequest], which must send one
/// request with a non-null body through a CBOR adapter — i.e. must reach
/// the generated runtime's `_encodeBody`, which is where the cached codec
/// future is awaited.
void runCodecMemoizationContract(
  String transport,
  Future<void> Function() issueRequest,
) {
  test('$transport: the codec future is memoized, and retried after a '
      'failure (cratestack#798)', () async {
    expect(stubCborCodecCalls, 0, reason: 'fresh isolate');

    // 1. A transient failure propagates to the caller rather than being
    //    swallowed — the fix must not turn an error into a silent null.
    stubCborCodecFailNext = true;
    await expectLater(issueRequest(), throwsA(isA<CratestackCborCodecError>()));
    expect(
      stubCborCodecCalls,
      1,
      reason: 'the failing attempt should have invoked the factory once',
    );

    // 2. THE REGRESSION. With a plain `??=` the rejected future stays
    //    cached, so this second request replays the same error forever and
    //    the factory is never called again — `stubCborCodecCalls` stays at
    //    1 and this line throws. Nothing about the failure was permanent;
    //    only the cache made it so.
    await issueRequest();
    expect(
      stubCborCodecCalls,
      2,
      reason:
          'a failed resolution must not stay cached — the next request has '
          'to get a fresh attempt, not a replay of the rejection',
    );

    // 3. ...and the memoization it replaced still holds, so the fix did
    //    not simply turn caching off. Three more requests, no new calls.
    await issueRequest();
    await issueRequest();
    await issueRequest();
    expect(
      stubCborCodecCalls,
      2,
      reason:
          'a SUCCESSFUL resolution must stay cached — the native library / '
          'wasm module is loaded once per isolate, not once per request',
    );
  });
}
