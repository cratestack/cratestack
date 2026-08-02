import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:fast_immutable_collections/fast_immutable_collections.dart';
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
  });

  final int? id;
  final String? name;

  factory Widget.fromWire(CratestackValueMap value) {
    return Widget(
      id: value['id'] == null ? null : (value['id'] as num).toInt(),
      name: value['name'] == null ? null : value['name'] as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
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
  });

  final int id;
  final String name;

  factory CreateWidgetInput.fromWire(CratestackValueMap value) {
    return CreateWidgetInput(
      id: (cratestackRequireWireValue('CreateWidgetInput', 'id', value['id']) as num).toInt(),
      name: cratestackRequireWireValue('CreateWidgetInput', 'name', value['name']) as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
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
  });

  final String? name;

  factory UpdateWidgetInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetInput(
      name: value['name'] == null ? null : value['name'] as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
    };
  }
}

class ProjectedWidget {
  const ProjectedWidget.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get name => _value['name'] == null ? null : _value['name'] as String;

}

class WidgetApi {
  const WidgetApi(this._client);

  final DartVerifyRiverpodCollisionCratestackClient _client;

  Future<IList<Widget>> list({
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/widgets',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Widget.fromWire(cratestackAsValueMap(item))).toIList();
  }

  Future<IList<T>> listView<T>({
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
        .toIList();
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

final dartVerifyRiverpodCollisionWidgetApiProvider = Provider<WidgetApi>((ref) {
  return ref.watch(dartVerifyRiverpodCollisionClientProvider).widgets;
});

// Issue #302: one `@riverpod` provider per operation, built by watching
// `dartVerifyRiverpodCollisionWidgetApiProvider` — the existing `Provider<WidgetApi>`
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
Future<Widget> widget(
  Ref ref,
  int id, {
  CratestackFetchQuery? query,
}) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetApiProvider).get(id, query: query);
}

@riverpod
Future<IList<Widget>> widgetList(Ref ref, {
  CratestackListQuery? query,
}) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetApiProvider).list(query: query);
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
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetApiProvider).create(input);
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
  // choice — `dartVerifyRiverpodCollisionWidgetApiProvider`'s own `.update(id, patch)`
  // call below is unaffected; only this controller's own method needed
  // renaming.
  Future<Widget> save(int id, UpdateWidgetInput patch) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetApiProvider).update(id, patch);
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
      final result = await ref.read(dartVerifyRiverpodCollisionWidgetApiProvider).delete(id);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}
