import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'models.dart';
import 'queries.dart';
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

final tinyRestClientWidgetApiProvider = Provider<WidgetApi>((ref) {
  return ref.watch(tinyRestClientClientProvider).widgets;
});

final tinyRestClientProceduresApiProvider = Provider<ProceduresApi>((ref) {
  return ref.watch(tinyRestClientClientProvider).procedures;
});

class WidgetApi {
  const WidgetApi(this._client);

  final TinyRestClientCratestackClient _client;

  Future<List<Widget>> list({
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widgets',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Widget.fromWire(cratestackAsValueMap(item))).toList(growable: false);
  }

  Future<List<T>> listView<T>({
    required CratestackProjection<T> projection,
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widgets',
      queryParameters: cratestackMergeFetchIntoListQuery(query, projection.toFetchQuery()).toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body)
        .map((item) => projection.fromWire(cratestackAsValueMap(item)))
        .toList(growable: false);
  }

  Future<Widget> get(int id, {
    CratestackFetchQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widgets/$id',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<T> getView<T>(int id, {
    required CratestackProjection<T> projection,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widgets/$id',
      queryParameters: projection.toFetchQuery().toQueryParameters(),
      options: options,
    );
    return projection.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> create(CreateWidgetInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/widgets',
      body: input.toWire(),
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> update(int id, UpdateWidgetInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'PATCH',
      '/widgets/$id',
      body: input.toWire(),
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }

  Future<Widget> delete(int id, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'DELETE',
      '/widgets/$id',
      options: options,
    );
    return Widget.fromWire(cratestackAsValueMap(body));
  }
}

class ProceduresApi {
  const ProceduresApi(this._client);

  final TinyRestClientCratestackClient _client;

  Future<String> echoName(EchoNameArgs args, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/\$procs/echoName',
      body: args.toWire(),
      options: options,
    );
    return cratestackRequireWireValue('Procedure', 'echoName', body) as String;
  }

}
