import 'models/widget.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class TinyRpcClientCratestackClient {
  TinyRpcClientCratestackClient(this._adapter);

  final CratestackRpcAdapter _adapter;

  CratestackRpcAdapter get adapter => _adapter;

  WidgetApi get widgets => WidgetApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

final tinyRpcClientAdapterProvider = Provider<CratestackRpcAdapter>((ref) {
  throw UnimplementedError('Override tinyRpcClientAdapterProvider before reading the generated CrateStack client.');
});

final tinyRpcClientClientProvider = Provider<TinyRpcClientCratestackClient>((ref) {
  return TinyRpcClientCratestackClient(ref.watch(tinyRpcClientAdapterProvider));
});
