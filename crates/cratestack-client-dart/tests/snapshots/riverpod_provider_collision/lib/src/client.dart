import 'models/widget.dart';
import 'models/widget_list.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'procedures.dart';
import 'runtime.dart';

class DartVerifyRiverpodCollisionCratestackClient {
  DartVerifyRiverpodCollisionCratestackClient(this._adapter, {this.basePath = '/api'});

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
  WidgetListApi get widgetLists => WidgetListApi(this);
  ProceduresApi get procedures => ProceduresApi(this);
}

final dartVerifyRiverpodCollisionAdapterProvider = Provider<CratestackClientAdapter>((ref) {
  throw UnimplementedError('Override dartVerifyRiverpodCollisionAdapterProvider before reading the generated CrateStack client.');
});

final dartVerifyRiverpodCollisionBasePathProvider = Provider<String>((ref) => '/api');

final dartVerifyRiverpodCollisionClientProvider = Provider<DartVerifyRiverpodCollisionCratestackClient>((ref) {
  return DartVerifyRiverpodCollisionCratestackClient(
    ref.watch(dartVerifyRiverpodCollisionAdapterProvider),
    basePath: ref.watch(dartVerifyRiverpodCollisionBasePathProvider),
  );
});
