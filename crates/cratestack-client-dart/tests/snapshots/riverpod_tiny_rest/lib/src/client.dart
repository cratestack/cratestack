import 'models/widget.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class TinyRestClientCratestackClient {
  TinyRestClientCratestackClient(this._adapter, {this.basePath = '/api'});

  final CratestackClientAdapter _adapter;
  final String basePath;

  Future<Object?> execute(
    String method,
    String path, {
    Object? body,
    Map<String, Object?>? queryParameters,
    CratestackCallOptions? options,
  }) {
    return _adapter.execute(
      CratestackRequest(
        method: method,
        path: '$basePath$path',
        queryParameters: queryParameters,
        body: body,
      ),
      options: options,
    );
  }

  WidgetApi get widgets => WidgetApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

final tinyRestClientAdapterProvider = Provider<CratestackClientAdapter>((ref) {
  throw UnimplementedError('Override tinyRestClientAdapterProvider before reading the generated CrateStack client.');
});

final tinyRestClientBasePathProvider = Provider<String>((ref) => '/api');

final tinyRestClientClientProvider = Provider<TinyRestClientCratestackClient>((ref) {
  return TinyRestClientCratestackClient(
    ref.watch(tinyRestClientAdapterProvider),
    basePath: ref.watch(tinyRestClientBasePathProvider),
  );
});
