import '../client.dart';
import '../runtime.dart';
import 'package:cratestack_annotations/cratestack_annotations.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:fast_immutable_collections/fast_immutable_collections.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'shared_types.dart';

part 'widget.g.dart';
part 'widget.mapper.dart';
part 'widget.builder.dart';

enum WidgetSortField {
  id('id'),  name('name'),  weight('weight');
  const WidgetSortField(this.wireName);

  final String wireName;

  static WidgetSortField fromWire(Object? value) {
    final wireName = value as String;
    switch (wireName) {
      case 'id':
        return WidgetSortField.id;
      case 'name':
        return WidgetSortField.name;
      case 'weight':
        return WidgetSortField.weight;
    }
    throw ArgumentError.value(wireName, 'value', 'Unknown WidgetSortField value');
  }

  Object toWire() => wireName;
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
@CratestackBuilder()
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
@CratestackBuilder()
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
@CratestackBuilder(listDefaults: false, touchFlagFields: {'weight'})
class UpdateWidgetInput with UpdateWidgetInputMappable {
  const UpdateWidgetInput({
this.name,
this.weight,
    this.weightIsSet = false,
  });

  final String? name;
  final int? weight;
  // Outer "did the caller touch this field" flag, alongside
  // `weight`'s own value (the inner "new value, or `null`
  // to clear") — the Dart analogue of the generated Rust client's
  // `Option<Option<T>>` for this nullable-column field (cratestack#663).
  // Only meaningful when `weight == null`: `false` there
  // means untouched (stays off the wire), `true` means an explicit clear
  // (serializes as `null`). A non-null `weight` always
  // serializes regardless of this flag — the plain `const` constructor is
  // public, so `UpdateWidgetInput(weight: value)`
  // (bypassing the builder, which is the only thing that otherwise sets
  // this flag) must still put a caller-supplied value on the wire.
  final bool weightIsSet;

  factory UpdateWidgetInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetInput(
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
      weightIsSet: value.containsKey('weight'),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      if (name != null) 'name': name,
      if (weightIsSet || weight != null) 'weight': weight,
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
@CratestackBuilder()
class WidgetWhere with WidgetWhereMappable {
  const WidgetWhere({
this.id,
this.name,
this.weight,
  });

  final NumberFilter? id;
  final StringFilter? name;
  final NumberFilter? weight;

  factory WidgetWhere.fromWire(CratestackValueMap value) {
    return WidgetWhere(
      id: value['id'] == null ? null : NumberFilter.fromWire(cratestackAsValueMap(value['id'])),
      name: value['name'] == null ? null : StringFilter.fromWire(cratestackAsValueMap(value['name'])),
      weight: value['weight'] == null ? null : NumberFilter.fromWire(cratestackAsValueMap(value['weight'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id?.toWire(),
      'name': name?.toWire(),
      'weight': weight?.toWire(),
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
@CratestackBuilder()
class WidgetOrderByClause with WidgetOrderByClauseMappable {
  const WidgetOrderByClause({
required this.field,
required this.direction,
  });

  final WidgetSortField field;
  final SortDirection direction;

  factory WidgetOrderByClause.fromWire(CratestackValueMap value) {
    return WidgetOrderByClause(
      field: WidgetSortField.fromWire(cratestackRequireWireValue('WidgetOrderByClause', 'field', value['field'])),
      direction: SortDirection.fromWire(cratestackRequireWireValue('WidgetOrderByClause', 'direction', value['direction'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'field': field.toWire(),
      'direction': direction.toWire(),
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
@CratestackBuilder()
class WidgetFindMany with WidgetFindManyMappable {
  const WidgetFindMany({
this.where,
this.orderBy,
  });

  final WidgetWhere? where;
  final List<WidgetOrderByClause>? orderBy;

  factory WidgetFindMany.fromWire(CratestackValueMap value) {
    return WidgetFindMany(
      where: value['where'] == null ? null : WidgetWhere.fromWire(cratestackAsValueMap(value['where'])),
      orderBy: value['orderBy'] == null ? null : cratestackAsValueList(value['orderBy']).map((item) => WidgetOrderByClause.fromWire(cratestackAsValueMap(item))).toList(growable: false),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'where': where?.toWire(),
      'orderBy': orderBy?.map((item) => item.toWire()).toList(growable: false),
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

  Future<IList<Widget>> list({
    Map<String, Object?> input = const <String, Object?>{},
    CratestackRpcCallOptions? options,
  }) async {
    final body = await _client.adapter.call(
      'model.Widget.list',
      input,
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Widget.fromWire(cratestackAsValueMap(item))).toIList();
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

// Issue #331: RPC transport has no typed query-builder class (unlike
// REST's `CratestackListQuery` — see this story's PR body for the
// explicit, documented decision). `WidgetApi.list()` itself still
// takes a bare `Map<String, Object?> input` (unchanged, `rpc_model.dart.j2`), but
// this provider's own parameter is `IMap<String, Object?>` — not
// `Map<String, Object?>` — because `Map`'s default `==` is
// identity-based (the exact caching bug just described for REST's
// `query` param above would reappear here otherwise); `IMap`
// (`fast_immutable_collections`, already a riverpod-preset dependency)
// has real value equality, so a `@riverpod` family lookup with a
// freshly-built-but-equal input actually dedupes.
@riverpod
Future<IList<Widget>> widgetList(Ref ref, {
  IMap<String, Object?>? input,
}) {
  return ref.watch(tinyRpcClientWidgetApiProvider).list(input: input?.unlock ?? const <String, Object?>{});
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
