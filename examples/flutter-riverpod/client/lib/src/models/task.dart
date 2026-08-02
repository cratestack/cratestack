import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'board.dart';
import 'package:dart_mappable/dart_mappable.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'task.g.dart';
part 'task.mapper.dart';

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
class Task with TaskMappable {
  const Task({
this.id,
this.title,
this.done,
this.boardId,
this.board,
  });

  final int? id;
  final String? title;
  final bool? done;
  final int? boardId;
  final Board? board;

  factory Task.fromWire(CratestackValueMap value) {
    return Task(
      id: value['id'] == null ? null : (value['id'] as num).toInt(),
      title: value['title'] == null ? null : value['title'] as String,
      done: value['done'] == null ? null : value['done'] as bool,
      boardId: value['boardId'] == null ? null : (value['boardId'] as num).toInt(),
      board: value['board'] == null ? null : Board.fromWire(cratestackAsValueMap(value['board'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'title': title,
      'done': done,
      'boardId': boardId,
      'board': board?.toWire(),
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
class CreateTaskInput with CreateTaskInputMappable {
  const CreateTaskInput({
required this.id,
required this.title,
required this.done,
required this.boardId,
  });

  final int id;
  final String title;
  final bool done;
  final int boardId;

  factory CreateTaskInput.fromWire(CratestackValueMap value) {
    return CreateTaskInput(
      id: (cratestackRequireWireValue('CreateTaskInput', 'id', value['id']) as num).toInt(),
      title: cratestackRequireWireValue('CreateTaskInput', 'title', value['title']) as String,
      done: cratestackRequireWireValue('CreateTaskInput', 'done', value['done']) as bool,
      boardId: (cratestackRequireWireValue('CreateTaskInput', 'boardId', value['boardId']) as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'title': title,
      'done': done,
      'boardId': boardId,
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
class UpdateTaskInput with UpdateTaskInputMappable {
  const UpdateTaskInput({
this.title,
this.done,
this.boardId,
  });

  final String? title;
  final bool? done;
  final int? boardId;

  factory UpdateTaskInput.fromWire(CratestackValueMap value) {
    return UpdateTaskInput(
      title: value['title'] == null ? null : value['title'] as String,
      done: value['done'] == null ? null : value['done'] as bool,
      boardId: value['boardId'] == null ? null : (value['boardId'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'title': title,
      'done': done,
      'boardId': boardId,
    };
  }
}

class ProjectedTask {
  const ProjectedTask.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get title => _value['title'] == null ? null : _value['title'] as String;

  bool? get done => _value['done'] == null ? null : _value['done'] as bool;

  int? get boardId => _value['boardId'] == null ? null : (_value['boardId'] as num).toInt();

  ProjectedBoard? get board {
    final value = _value['board'];
    if (value == null) return null;
    return ProjectedBoard.fromWire(cratestackAsValueMap(value));
  }

}

class TaskApi {
  const TaskApi(this._client);

  final FlutterRiverpodClientCratestackClient _client;

  Future<List<Task>> list({
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/tasks',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Task.fromWire(cratestackAsValueMap(item))).toList(growable: false);
  }

  Future<List<T>> listView<T>({
    required CratestackProjection<T> projection,
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/tasks',
      queryParameters: cratestackMergeFetchIntoListQuery(query, projection.toFetchQuery()).toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body)
        .map((item) => projection.fromWire(cratestackAsValueMap(item)))
        .toList(growable: false);
  }

  Future<Task> get(int id, {
    CratestackFetchQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/tasks/$id',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return Task.fromWire(cratestackAsValueMap(body));
  }

  Future<T> getView<T>(int id, {
    required CratestackProjection<T> projection,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/tasks/$id',
      queryParameters: projection.toFetchQuery().toQueryParameters(),
      options: options,
    );
    return projection.fromWire(cratestackAsValueMap(body));
  }

  Future<Task> create(CreateTaskInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/tasks',
      body: input.toWire(),
      options: options,
    );
    return Task.fromWire(cratestackAsValueMap(body));
  }

  Future<Task> update(int id, UpdateTaskInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'PATCH',
      '/tasks/$id',
      body: input.toWire(),
      options: options,
    );
    return Task.fromWire(cratestackAsValueMap(body));
  }

  Future<Task> delete(int id, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'DELETE',
      '/tasks/$id',
      options: options,
    );
    return Task.fromWire(cratestackAsValueMap(body));
  }
}

final flutterRiverpodClientTaskApiProvider = Provider<TaskApi>((ref) {
  return ref.watch(flutterRiverpodClientClientProvider).tasks;
});

// Issue #302: one `@riverpod` provider per operation, built by watching
// `flutterRiverpodClientTaskApiProvider` — the existing `Provider<TaskApi>`
// relocated by #301 (right above this block) — never the adapter/client
// providers in `client.dart` directly. Overriding
// `flutterRiverpodClientAdapterProvider` alone (the pre-existing Dio
// override point) is enough to change what every provider below does.

@riverpod
Future<Task> task(Ref ref, int id) {
  return ref.watch(flutterRiverpodClientTaskApiProvider).get(id);
}

@riverpod
Future<List<Task>> taskList(Ref ref) {
  return ref.watch(flutterRiverpodClientTaskApiProvider).list();
}

// Writes are controllers, not `FutureProvider`s: a mutation isn't a value
// to cache and re-fetch on every listener, it's an action with its own
// loading/error/success lifecycle. `AsyncNotifier`'s `state` gives
// widgets that lifecycle for free (`.isLoading`, `.hasError`, `.value`)
// while the method itself still returns the created/updated/deleted
// record directly for callers that just want the result.

@riverpod
class TaskCreateController extends _$TaskCreateController {
  @override
  FutureOr<Task?> build() => null;

  Future<Task> create(CreateTaskInput input) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientTaskApiProvider).create(input);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class TaskUpdateController extends _$TaskUpdateController {
  @override
  FutureOr<Task?> build() => null;

  // Named `save`, not `update`: `AsyncNotifier`/`_$AsyncClassModifier`
  // (the riverpod_generator-produced base class) already declares its
  // own `update(FutureOr<ValueT> Function(ValueT) cb, {onError})` method
  // for mutating `state` from its previous value. A same-named override
  // here with an incompatible signature is a real `dart analyze`
  // `invalid_override` error (confirmed empirically), not a style
  // choice — `flutterRiverpodClientTaskApiProvider`'s own `.update(id, patch)`
  // call below is unaffected; only this controller's own method needed
  // renaming.
  Future<Task> save(int id, UpdateTaskInput patch) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientTaskApiProvider).update(id, patch);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class TaskDeleteController extends _$TaskDeleteController {
  @override
  FutureOr<Task?> build() => null;

  Future<Task> delete(int id) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientTaskApiProvider).delete(id);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}
