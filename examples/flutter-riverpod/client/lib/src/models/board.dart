import '../client.dart';
import '../queries.dart';
import '../runtime.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'board.g.dart';

class Board {
  const Board({
this.id,
this.name,
  });

  final int? id;
  final String? name;

  factory Board.fromWire(CratestackValueMap value) {
    return Board(
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

class CreateBoardInput {
  const CreateBoardInput({
required this.id,
required this.name,
  });

  final int id;
  final String name;

  factory CreateBoardInput.fromWire(CratestackValueMap value) {
    return CreateBoardInput(
      id: (cratestackRequireWireValue('CreateBoardInput', 'id', value['id']) as num).toInt(),
      name: cratestackRequireWireValue('CreateBoardInput', 'name', value['name']) as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
    };
  }
}

class UpdateBoardInput {
  const UpdateBoardInput({
this.name,
  });

  final String? name;

  factory UpdateBoardInput.fromWire(CratestackValueMap value) {
    return UpdateBoardInput(
      name: value['name'] == null ? null : value['name'] as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
    };
  }
}

class ProjectedBoard {
  const ProjectedBoard.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get name => _value['name'] == null ? null : _value['name'] as String;

}

class BoardApi {
  const BoardApi(this._client);

  final FlutterRiverpodClientCratestackClient _client;

  Future<List<Board>> list({
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/boards',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body).map((item) => Board.fromWire(cratestackAsValueMap(item))).toList(growable: false);
  }

  Future<List<T>> listView<T>({
    required CratestackProjection<T> projection,
    CratestackListQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/boards',
      queryParameters: cratestackMergeFetchIntoListQuery(query, projection.toFetchQuery()).toQueryParameters(),
      options: options,
    );
    return cratestackAsValueList(body)
        .map((item) => projection.fromWire(cratestackAsValueMap(item)))
        .toList(growable: false);
  }

  Future<Board> get(int id, {
    CratestackFetchQuery? query,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/boards/$id',
      queryParameters: query?.toQueryParameters(),
      options: options,
    );
    return Board.fromWire(cratestackAsValueMap(body));
  }

  Future<T> getView<T>(int id, {
    required CratestackProjection<T> projection,
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'GET',
      '/boards/$id',
      queryParameters: projection.toFetchQuery().toQueryParameters(),
      options: options,
    );
    return projection.fromWire(cratestackAsValueMap(body));
  }

  Future<Board> create(CreateBoardInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'POST',
      '/boards',
      body: input.toWire(),
      options: options,
    );
    return Board.fromWire(cratestackAsValueMap(body));
  }

  Future<Board> update(int id, UpdateBoardInput input, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'PATCH',
      '/boards/$id',
      body: input.toWire(),
      options: options,
    );
    return Board.fromWire(cratestackAsValueMap(body));
  }

  Future<Board> delete(int id, {
    CratestackCallOptions? options,
  }) async {
    final body = await _client.execute(
      'DELETE',
      '/boards/$id',
      options: options,
    );
    return Board.fromWire(cratestackAsValueMap(body));
  }
}

final flutterRiverpodClientBoardApiProvider = Provider<BoardApi>((ref) {
  return ref.watch(flutterRiverpodClientClientProvider).boards;
});

// Issue #302: one `@riverpod` provider per operation, built by watching
// `flutterRiverpodClientBoardApiProvider` — the existing `Provider<BoardApi>`
// relocated by #301 (right above this block) — never the adapter/client
// providers in `client.dart` directly. Overriding
// `flutterRiverpodClientAdapterProvider` alone (the pre-existing Dio
// override point) is enough to change what every provider below does.

@riverpod
Future<Board> board(Ref ref, int id) {
  return ref.watch(flutterRiverpodClientBoardApiProvider).get(id);
}

@riverpod
Future<List<Board>> boardList(Ref ref) {
  return ref.watch(flutterRiverpodClientBoardApiProvider).list();
}

// Writes are controllers, not `FutureProvider`s: a mutation isn't a value
// to cache and re-fetch on every listener, it's an action with its own
// loading/error/success lifecycle. `AsyncNotifier`'s `state` gives
// widgets that lifecycle for free (`.isLoading`, `.hasError`, `.value`)
// while the method itself still returns the created/updated/deleted
// record directly for callers that just want the result.

@riverpod
class BoardCreateController extends _$BoardCreateController {
  @override
  FutureOr<Board?> build() => null;

  Future<Board> create(CreateBoardInput input) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientBoardApiProvider).create(input);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class BoardUpdateController extends _$BoardUpdateController {
  @override
  FutureOr<Board?> build() => null;

  // Named `save`, not `update`: `AsyncNotifier`/`_$AsyncClassModifier`
  // (the riverpod_generator-produced base class) already declares its
  // own `update(FutureOr<ValueT> Function(ValueT) cb, {onError})` method
  // for mutating `state` from its previous value. A same-named override
  // here with an incompatible signature is a real `dart analyze`
  // `invalid_override` error (confirmed empirically), not a style
  // choice — `flutterRiverpodClientBoardApiProvider`'s own `.update(id, patch)`
  // call below is unaffected; only this controller's own method needed
  // renaming.
  Future<Board> save(int id, UpdateBoardInput patch) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientBoardApiProvider).update(id, patch);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}

@riverpod
class BoardDeleteController extends _$BoardDeleteController {
  @override
  FutureOr<Board?> build() => null;

  Future<Board> delete(int id) async {
    state = const AsyncValue.loading();
    try {
      final result = await ref.read(flutterRiverpodClientBoardApiProvider).delete(id);
      state = AsyncValue.data(result);
      return result;
    } catch (error, stackTrace) {
      state = AsyncValue.error(error, stackTrace);
      rethrow;
    }
  }
}
