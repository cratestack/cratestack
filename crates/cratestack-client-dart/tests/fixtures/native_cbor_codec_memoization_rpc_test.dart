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
import 'package:native_cbor_memo_rpc/src/runtime.dart';

import 'native_cbor_codec_memoization_common.dart';

void main() {
  final adapter = CratestackRpcCborDioAdapter(dio: stubDio());

  runCodecMemoizationContract(
    'RPC',
    // Non-null input for the same reason the REST half passes a body:
    // `_encodeBody` short-circuits on null and never resolves the codec.
    () => adapter.call('procedure.echo', <String, Object?>{'name': 'thing'}),
  );
}
