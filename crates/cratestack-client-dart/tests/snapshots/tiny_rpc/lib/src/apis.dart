import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'models.dart';
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

final tinyRpcClientWidgetApiProvider = Provider<WidgetApi>((ref) {
  return ref.watch(tinyRpcClientClientProvider).widgets;
});

final tinyRpcClientProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(tinyRpcClientClientProvider).procedures;
});

class WidgetApi {
  const WidgetApi(this._client);

  final TinyRpcClientCratestackClient _client;

  Future<List<Widget>> list({
    Map<String, Object?> input = const <String, Object?>{},
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.list',
      input,
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Widget.fromWire(cratestackAsValueMap(item))).toList(growable: false);
  }

  Future<Widget> get(int id, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.get',
      {'id': id},
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> create(CreateWidgetInput input, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.create',
      input.toWire(),
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> update(int id, UpdateWidgetInput patch, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.update',
      {'id': id, 'patch': patch.toWire()},
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> delete(int id, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.delete',
      {'id': id},
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }
}

class ProceduresApi {
  const ProceduresApi(this._client);

  final TinyRpcClientCratestackClient _client;

  Future<String> echoName(EchoNameArgs args, {
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'procedure.echoName',
      args.toWire(),
      options: options,
    );
    return cratestackRequireWireValue('Procedure', 'echoName', body) as String;
  }

}