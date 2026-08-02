import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'package:fast_immutable_collections/fast_immutable_collections.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'widget_list.g.dart';

class WidgetList {
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

class CreateWidgetListInput {
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

class UpdateWidgetListInput {
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

@riverpod
Future<WidgetList> dartVerifyRiverpodCollisionWidgetList(Ref ref, int id) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).get(id);
}

@riverpod
Future<IList<WidgetList>> widgetListList(Ref ref) {
  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).list();
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
