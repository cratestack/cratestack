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
import 'package:dio/dio.dart';
import 'package:native_cbor_memo_rest/src/runtime.dart';

import 'native_cbor_codec_memoization_common.dart';

void main() {
  final adapter = CratestackCborDioAdapter(dio: stubDio());

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
}
