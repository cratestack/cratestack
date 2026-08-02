import '../client.dart';
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

final tinyRpcClientWidgetApiProvider = Provider<WidgetApi>((ref) {
  return ref.watch(tinyRpcClientClientProvider).widgets;
});
