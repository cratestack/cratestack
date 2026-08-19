import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:fast_immutable_collections/fast_immutable_collections.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'shared_types.dart';

part 'widget_list.g.dart';
part 'widget_list.mapper.dart';

enum WidgetListSortField {
  id('id'),  label('label');
  const WidgetListSortField(this.wireName);

  final String wireName;

  static WidgetListSortField fromWire(Object? value) {
    final wireName = value as String;
    switch (wireName) {
      case 'id':
        return WidgetListSortField.id;
      case 'label':
        return WidgetListSortField.label;
    }
    throw ArgumentError.value(wireName, 'value', 'Unknown WidgetListSortField value');
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
class WidgetList with WidgetListMappable {
  const WidgetList({
this.id,
this.label,
  });

  final int? id;
  final String? label;

  factory WidgetList.fromWire(CratestackValueMap value) {
    return WidgetList(
      id: value['id'] == null ? null : (value['id'] as num).toInt(),
      label: value['label'] == null ? null : value['label'] as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'label': label,
    };
  }
}

class WidgetListBuilder {
  int? _id;
  String? _label;

  WidgetListBuilder id(int? value) {
    _id = value;
    return this;
  }

  WidgetListBuilder label(String? value) {
    _label = value;
    return this;
  }

  WidgetList build() {
    return WidgetList(
      id: _id,
      label: _label,
    );
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
class CreateWidgetListInput with CreateWidgetListInputMappable {
  const CreateWidgetListInput({
required this.id,
required this.label,
  });

  final int id;
  final String label;

  factory CreateWidgetListInput.fromWire(CratestackValueMap value) {
    return CreateWidgetListInput(
      id: (cratestackRequireWireValue('CreateWidgetListInput', 'id', value['id']) as num).toInt(),
      label: cratestackRequireWireValue('CreateWidgetListInput', 'label', value['label']) as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'label': label,
    };
  }
}

class CreateWidgetListInputBuilder {
  int? _id;
  bool _idSet = false;
  String? _label;
  bool _labelSet = false;

  CreateWidgetListInputBuilder id(int value) {
    _id = value;
    _idSet = true;
    return this;
  }

  CreateWidgetListInputBuilder label(String value) {
    _label = value;
    _labelSet = true;
    return this;
  }

  CreateWidgetListInput build() {
    return CreateWidgetListInput(
      id: _idSet ? (_id as int) : (throw StateError('CreateWidgetListInput.id is required but was not set')),
      label: _labelSet ? (_label as String) : (throw StateError('CreateWidgetListInput.label is required but was not set')),
    );
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
class UpdateWidgetListInput with UpdateWidgetListInputMappable {
  const UpdateWidgetListInput({
this.label,
  });

  final String? label;

  factory UpdateWidgetListInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetListInput(
      label: value['label'] == null ? null : value['label'] as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'label': label,
    };
  }
}

class UpdateWidgetListInputBuilder {
  String? _label;

  UpdateWidgetListInputBuilder label(String? value) {
    _label = value;
    return this;
  }

  UpdateWidgetListInput build() {
    return UpdateWidgetListInput(
      label: _label,
    );
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
class WidgetListWhere with WidgetListWhereMappable {
  const WidgetListWhere({
this.id,
this.label,
  });

  final NumberFilter? id;
  final StringFilter? label;

  factory WidgetListWhere.fromWire(CratestackValueMap value) {
    return WidgetListWhere(
      id: value['id'] == null ? null : NumberFilter.fromWire(cratestackAsValueMap(value['id'])),
      label: value['label'] == null ? null : StringFilter.fromWire(cratestackAsValueMap(value['label'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id?.toWire(),
      'label': label?.toWire(),
    };
  }
}

class WidgetListWhereBuilder {
  NumberFilter? _id;
  StringFilter? _label;

  WidgetListWhereBuilder id(NumberFilter? value) {
    _id = value;
    return this;
  }

  WidgetListWhereBuilder label(StringFilter? value) {
    _label = value;
    return this;
  }

  WidgetListWhere build() {
    return WidgetListWhere(
      id: _id,
      label: _label,
    );
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
class WidgetListOrderByClause with WidgetListOrderByClauseMappable {
  const WidgetListOrderByClause({
required this.field,
required this.direction,
  });

  final WidgetListSortField field;
  final SortDirection direction;

  factory WidgetListOrderByClause.fromWire(CratestackValueMap value) {
    return WidgetListOrderByClause(
      field: WidgetListSortField.fromWire(cratestackRequireWireValue('WidgetListOrderByClause', 'field', value['field'])),
      direction: SortDirection.fromWire(cratestackRequireWireValue('WidgetListOrderByClause', 'direction', value['direction'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'field': field.toWire(),
      'direction': direction.toWire(),
    };
  }
}

class WidgetListOrderByClauseBuilder {
  WidgetListSortField? _field;
  bool _fieldSet = false;
  SortDirection? _direction;
  bool _directionSet = false;

  WidgetListOrderByClauseBuilder field(WidgetListSortField value) {
    _field = value;
    _fieldSet = true;
    return this;
  }

  WidgetListOrderByClauseBuilder direction(SortDirection value) {
    _direction = value;
    _directionSet = true;
    return this;
  }

  WidgetListOrderByClause build() {
    return WidgetListOrderByClause(
      field: _fieldSet ? (_field as WidgetListSortField) : (throw StateError('WidgetListOrderByClause.field is required but was not set')),
      direction: _directionSet ? (_direction as SortDirection) : (throw StateError('WidgetListOrderByClause.direction is required but was not set')),
    );
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
class WidgetListFindMany with WidgetListFindManyMappable {
  const WidgetListFindMany({
this.where,
this.orderBy,
  });

  final WidgetListWhere? where;
  final List<WidgetListOrderByClause>? orderBy;

  factory WidgetListFindMany.fromWire(CratestackValueMap value) {
    return WidgetListFindMany(
      where: value['where'] == null ? null : WidgetListWhere.fromWire(cratestackAsValueMap(value['where'])),
      orderBy: value['orderBy'] == null ? null : cratestackAsValueList(value['orderBy']).map((item) => WidgetListOrderByClause.fromWire(cratestackAsValueMap(item))).toList(growable: false),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'where': where?.toWire(),
      'orderBy': orderBy?.map((item) => item.toWire()).toList(growable: false),
    };
  }
}

class WidgetListFindManyBuilder {
  WidgetListWhere? _where;
  List<WidgetListOrderByClause>? _orderBy;

  WidgetListFindManyBuilder where(WidgetListWhere? value) {
    _where = value;
    return this;
  }

  WidgetListFindManyBuilder orderBy(List<WidgetListOrderByClause>? value) {
    _orderBy = value;
    return this;
  }

  WidgetListFindMany build() {
    return WidgetListFindMany(
      where: _where,
      orderBy: _orderBy,
    );
  }
}

class ProjectedWidgetList {
  const ProjectedWidgetList.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get label => _value['label'] == null ? null : _value['label'] as String;

}

class WidgetListApi {
  const WidgetListApi(this._client);

  final DartVerifyRiverpodCollisionCratestackClient _client;

  Future<IList<WidgetList>> list({
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widget_lists',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body).map((item) => WidgetList.fromWire(cratestackAsValueMap(item))).toIList();
  }

  Future<IList<T>> listView<T>({
    required CratestackProjection<T> projection,
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widget_lists',
      queryParameters: cratestackMergeFetchIntoListQuery(query, projection.toFetchQuery()).toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body)
        .map((item) => projection.fromWire(cratestackAsValueMap(item)))
        .toIList();
  }

  Future<WidgetList> get(int id, {
    CratestackFetchQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widget_lists/$id',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return WidgetList.fromWire(cratestackAsValueMap(body));
  }

  Future<T> getView<T>(int id, {
    required CratestackProjection<T> projection,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widget_lists/$id',
      queryParameters: projection.toFetchQuery().toQueryParameters(),
      options: options,
    );
    return projection.fromWire(cratestackAsValueMap(body));
  }

  Future<WidgetList> create(CreateWidgetListInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/widget_lists',
      body: input.toWire(),
      options: options,
    );
    return WidgetList.fromWire(cratestackAsValueMap(body));
  }

  Future<WidgetList> update(int id, UpdateWidgetListInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'PATCH',
      '/widget_lists/$id',
      body: input.toWire(),
      options: options,
    );
    return WidgetList.fromWire(cratestackAsValueMap(body));
  }

  Future<WidgetList> delete(int id, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'DELETE',
      '/widget_lists/$id',
      options: options,
    );
    return WidgetList.fromWire(cratestackAsValueMap(body));
  }
}

final dartVerifyRiverpodCollisionWidgetListApiProvider = Provider<WidgetListApi>((ref) {
  return ref.watch(dartVerifyRiverpodCollisionClientProvider).widgetLists;
});

// Issue #302: one `@riverpod` provider per operation, built by watching
// `dartVerifyRiverpodCollisionWidgetListApiProvider` — the existing `Provider<WidgetListApi>`
// relocated by #301 (right above this block) — never the adapter/client
// providers in `client.dart` directly. Overriding
// `dartVerifyRiverpodCollisionAdapterProvider` alone (the pre-existing Dio
// override point) is enough to change what every provider below does.

// Issue #331: `query` forwards straight to the underlying `XApi`
// method's own `CratestackFetchQuery?`/`CratestackListQuery?` parameter
// — the same fully-featured filter/pagination/sort/field-selection
// builder `rest-queries.dart.j2` already gives the plain, non-Riverpod
// client. Both query classes carry hand-rolled `operator ==`/
// `hashCode` (see `rest-queries.dart.j2`'s own comment) specifically so
// this works as a `@riverpod` family argument: a freshly-constructed
// query with the same values as a previous one must be `==` to it, or
// Riverpod's family cache never dedupes and the provider restarts
// `AsyncLoading` on every rebuild.
@riverpod
Future<WidgetList> dartVerifyRiverpodCollisionWidgetList(
  Ref ref,
  int id, {
  CratestackFetchQuery? query,
}) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).get(id, query: query);
}

@riverpod
Future<IList<WidgetList>> widgetListList(Ref ref, {
  CratestackListQuery? query,
}) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).list(query: query);
}

// Writes are controllers, not `FutureProvider`s: a mutation isn't a value
// to cache and re-fetch on every listener, it's an action with its own
// loading/error/success lifecycle. `AsyncNotifier`'s `state` gives
// widgets that lifecycle for free (`.isLoading`, `.hasError`, `.value`)
// while the method itself still returns the created/updated/deleted
// record directly for callers that just want the result.

@riverpod
class WidgetListCreateController extends _$WidgetListCreateController {
  @override
  FutureOr<WidgetList?> build() => null;

  Future<WidgetList> create(CreateWidgetListInput input) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetListApiProvider).create(input);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class WidgetListUpdateController extends _$WidgetListUpdateController {
  @override
  FutureOr<WidgetList?> build() => null;

  // Named `save`, not `update`: `AsyncNotifier`/`_$AsyncClassModifier`
  // (the riverpod_generator-produced base class) already declares its
  // own `update(FutureOr<ValueT> Function(ValueT) cb, {onError})` method
  // for mutating `state` from its previous value. A same-named override
  // here with an incompatible signature is a real `dart analyze`
  // `invalid_override` error (confirmed empirically), not a style
  // choice — `dartVerifyRiverpodCollisionWidgetListApiProvider`'s own `.update(id, patch)`
  // call below is unaffected; only this controller's own method needed
  // renaming.
  Future<WidgetList> save(int id, UpdateWidgetListInput patch) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetListApiProvider).update(id, patch);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class WidgetListDeleteController extends _$WidgetListDeleteController {
  @override
  FutureOr<WidgetList?> build() => null;

  Future<WidgetList> delete(int id) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetListApiProvider).delete(id);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}
