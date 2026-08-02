import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

class Widget {
  const Widget({
this.id,
this.name,
this.weight,
  });

  final int? id;
  final String? name;
  final int? weight;

  factory Widget.fromWire(CratestackValueMap value) {
    return Widget(
      id: value['id'] == null ? null : (value['id'] as num).toInt(),
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'weight': weight,
    };
  }
}

class CreateWidgetInput {
  const CreateWidgetInput({
required this.id,
required this.name,
this.weight,
  });

  final int id;
  final String name;
  final int? weight;

  factory CreateWidgetInput.fromWire(CratestackValueMap value) {
    return CreateWidgetInput(
      id: (cratestackRequireWireValue('CreateWidgetInput', 'id', value['id']) as num).toInt(),
      name: cratestackRequireWireValue('CreateWidgetInput', 'name', value['name']) as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'weight': weight,
    };
  }
}

class UpdateWidgetInput {
  const UpdateWidgetInput({
this.name,
this.weight,
  });

  final String? name;
  final int? weight;

  factory UpdateWidgetInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetInput(
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
      'weight': weight,
    };
  }
}

class ProjectedWidget {
  const ProjectedWidget.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get name => _value['name'] == null ? null : _value['name'] as String;

  int? get weight => _value['weight'] == null ? null : (_value['weight'] as num).toInt();

}

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

final tinyRestClientWidgetApiProvider = Provider<WidgetApi>((ref) {
  return ref.watch(tinyRestClientClientProvider).widgets;
});
