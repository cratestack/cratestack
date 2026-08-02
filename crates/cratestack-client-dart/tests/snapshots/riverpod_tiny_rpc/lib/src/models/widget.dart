import '../client.dart';
import '../runtime.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'widget.g.dart';
part 'widget.mapper.dart';

// issue #325: `@MappableClass()` (expanded by `dart_mappable_builder`
// alongside `riverpod_generator` in the same `build_runner` pass) gives
// this class real `operator ==`/`hashCode`/`copyWith` — every generated
// data class needs this under the `riverpod` preset because any of them
// can end up as a `@riverpod` family provider's argument type (see
// `EstimateFocusMinutesArgs` in `procedures.dart`), and riverpod's family
// cache dedupes provider instances by argument *value* equality, not
// identity. `generateMethods: GenerateMethods.equals | GenerateMethods.copy`
// deliberately does NOT ask for `encode`/`decode`/`stringify` — this
// generator already hand-rolls `fromWire`/`toWire` below (different
// method names, so there's no collision either way), and duplicating a
// second, unused `toMap`/`fromMap`/`toJson`/`fromJson` surface per class
// would be pure noise. Relation fields (e.g. `Task.board` -> `Board?`)
// get deep equality "for free": the referenced type is itself a
// `@MappableClass()`-annotated generated class in this same preset, so
// the field-by-field comparison this class's own `==` performs recurses
// into the related type's generated `==` rather than falling back to
// object identity; list-valued relations get the same element-wise
// (not `List.==`/identity) comparison automatically from
// `dart_mappable`'s own list handling.
@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)
class Widget with WidgetMappable {
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

// issue #325: `@MappableClass()` (expanded by `dart_mappable_builder`
// alongside `riverpod_generator` in the same `build_runner` pass) gives
// this class real `operator ==`/`hashCode`/`copyWith` — every generated
// data class needs this under the `riverpod` preset because any of them
// can end up as a `@riverpod` family provider's argument type (see
// `EstimateFocusMinutesArgs` in `procedures.dart`), and riverpod's family
// cache dedupes provider instances by argument *value* equality, not
// identity. `generateMethods: GenerateMethods.equals | GenerateMethods.copy`
// deliberately does NOT ask for `encode`/`decode`/`stringify` — this
// generator already hand-rolls `fromWire`/`toWire` below (different
// method names, so there's no collision either way), and duplicating a
// second, unused `toMap`/`fromMap`/`toJson`/`fromJson` surface per class
// would be pure noise. Relation fields (e.g. `Task.board` -> `Board?`)
// get deep equality "for free": the referenced type is itself a
// `@MappableClass()`-annotated generated class in this same preset, so
// the field-by-field comparison this class's own `==` performs recurses
// into the related type's generated `==` rather than falling back to
// object identity; list-valued relations get the same element-wise
// (not `List.==`/identity) comparison automatically from
// `dart_mappable`'s own list handling.
@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)
class CreateWidgetInput with CreateWidgetInputMappable {
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

// issue #325: `@MappableClass()` (expanded by `dart_mappable_builder`
// alongside `riverpod_generator` in the same `build_runner` pass) gives
// this class real `operator ==`/`hashCode`/`copyWith` — every generated
// data class needs this under the `riverpod` preset because any of them
// can end up as a `@riverpod` family provider's argument type (see
// `EstimateFocusMinutesArgs` in `procedures.dart`), and riverpod's family
// cache dedupes provider instances by argument *value* equality, not
// identity. `generateMethods: GenerateMethods.equals | GenerateMethods.copy`
// deliberately does NOT ask for `encode`/`decode`/`stringify` — this
// generator already hand-rolls `fromWire`/`toWire` below (different
// method names, so there's no collision either way), and duplicating a
// second, unused `toMap`/`fromMap`/`toJson`/`fromJson` surface per class
// would be pure noise. Relation fields (e.g. `Task.board` -> `Board?`)
// get deep equality "for free": the referenced type is itself a
// `@MappableClass()`-annotated generated class in this same preset, so
// the field-by-field comparison this class's own `==` performs recurses
// into the related type's generated `==` rather than falling back to
// object identity; list-valued relations get the same element-wise
// (not `List.==`/identity) comparison automatically from
// `dart_mappable`'s own list handling.
@MappableClass(generateMethods: GenerateMethods.equals | GenerateMethods.copy)
class UpdateWidgetInput with UpdateWidgetInputMappable {
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

// Issue #302: one `@riverpod` provider per operation, built by watching
// `tinyRpcClientWidgetApiProvider` — the existing `Provider<WidgetApi>`
// relocated by #301 (right above this block) — never the adapter/client
// providers in `client.dart` directly. Overriding
// `tinyRpcClientAdapterProvider` alone (the pre-existing Dio
// override point) is enough to change what every provider below does.

@riverpod
Future<Widget> widget(Ref ref, int id) {
  return ref.watch(tinyRpcClientWidgetApiProvider).get(id);
}

@riverpod
Future<List<Widget>> widgetList(Ref ref) {
  return ref.watch(tinyRpcClientWidgetApiProvider).list();
}

// Writes are controllers, not `FutureProvider`s: a mutation isn't a value
// to cache and re-fetch on every listener, it's an action with its own
// loading/error/success lifecycle. `AsyncNotifier`'s `state` gives
// widgets that lifecycle for free (`.isLoading`, `.hasError`, `.value`)
// while the method itself still returns the created/updated/deleted
// record directly for callers that just want the result.

@riverpod
class WidgetCreateController extends _$WidgetCreateController {
  @override
  FutureOr<Widget?> build() => null;

  Future<Widget> create(CreateWidgetInput input) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(tinyRpcClientWidgetApiProvider).create(input);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class WidgetUpdateController extends _$WidgetUpdateController {
  @override
  FutureOr<Widget?> build() => null;

  // Named `save`, not `update`: `AsyncNotifier`/`_$AsyncClassModifier`
  // (the riverpod_generator-produced base class) already declares its
  // own `update(FutureOr<ValueT> Function(ValueT) cb, {onError})` method
  // for mutating `state` from its previous value. A same-named override
  // here with an incompatible signature is a real `dart analyze`
  // `invalid_override` error (confirmed empirically), not a style
  // choice — `tinyRpcClientWidgetApiProvider`'s own `.update(id, patch)`
  // call below is unaffected; only this controller's own method needed
  // renaming.
  Future<Widget> save(int id, UpdateWidgetInput patch) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(tinyRpcClientWidgetApiProvider).update(id, patch);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class WidgetDeleteController extends _$WidgetDeleteController {
  @override
  FutureOr<Widget?> build() => null;

  Future<Widget> delete(int id) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(tinyRpcClientWidgetApiProvider).delete(id);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}
