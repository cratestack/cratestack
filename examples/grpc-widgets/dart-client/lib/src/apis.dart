import 'package:grpc/grpc.dart';

import 'models.dart';
import 'runtime.dart';

// Field-number tables sourced from the schema's `.pb.lock` at generation
// time (`docs/design/protobuf.md` §3.3) — the same numbers the Rust
// server's mirror structs and `.proto` artifact use, so this client's
// wire bytes decode correctly on the real server.
const CratestackGrpcMessageRegistry _messages = {
  'CreateWidgetInput': [
    CratestackGrpcFieldDescriptor(property: 'id', number: 1, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'name', number: 2, kind: 'string', repeated: false),
  ],
  'PageInfo': [
    CratestackGrpcFieldDescriptor(property: 'limit', number: 1, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'offset', number: 2, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'hasNextPage', number: 3, kind: 'bool', repeated: false, defaultsWhenAbsent: true),
    CratestackGrpcFieldDescriptor(property: 'hasPreviousPage', number: 4, kind: 'bool', repeated: false, defaultsWhenAbsent: true),
  ],
  'PageOfWidget': [
    CratestackGrpcFieldDescriptor(property: 'items', number: 1, kind: 'message', repeated: true, refName: 'Widget'),
    CratestackGrpcFieldDescriptor(property: 'totalCount', number: 2, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'pageInfo', number: 3, kind: 'message', repeated: false, refName: 'PageInfo'),
  ],
  'UpdateWidgetInput': [
    CratestackGrpcFieldDescriptor(property: 'name', number: 1, kind: 'string', repeated: false),
  ],
  'Widget': [
    CratestackGrpcFieldDescriptor(property: 'id', number: 1, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'name', number: 2, kind: 'string', repeated: false),
  ],
  'WidgetRpcListInput': [
    CratestackGrpcFieldDescriptor(property: 'limit', number: 1, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'offset', number: 2, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'fields', number: 3, kind: 'string', repeated: true),
    CratestackGrpcFieldDescriptor(property: 'include', number: 4, kind: 'string', repeated: true),
    CratestackGrpcFieldDescriptor(property: 'sort', number: 6, kind: 'string', repeated: false),
  ],
  'WidgetRpcPkInput': [
    CratestackGrpcFieldDescriptor(property: 'id', number: 1, kind: 'int64', repeated: false),
  ],
  'WidgetRpcUpdateInput': [
    CratestackGrpcFieldDescriptor(property: 'id', number: 1, kind: 'int64', repeated: false),
    CratestackGrpcFieldDescriptor(property: 'patch', number: 2, kind: 'message', repeated: false, refName: 'UpdateWidgetInput'),
  ],
};

const CratestackGrpcEnumRegistry _enums = {
};

class GrpcWidgetsClientCratestackClient {
  GrpcWidgetsClientCratestackClient(this.runtime);

  final CratestackGrpcRuntime runtime;
  late final WidgetApi widgets = WidgetApi(runtime);
}

/// `list()`'s input covers the common list-projection controls
/// (`limit`/`offset`/`fields`/`include`/`sort`) — raw predicate queries
/// (`where`/`or`/structured filters) and per-relation field projection
/// (`includeFields`) are not wired into the generated gRPC client in this
/// pass; see this ticket's final report.
class CratestackGrpcListInput {
  const CratestackGrpcListInput({
    this.limit,
    this.offset,
    this.fields = const <String>[],
    this.include = const <String>[],
    this.sort,
  });

  final int? limit;
  final int? offset;
  final List<String> fields;
  final List<String> include;
  final String? sort;

  CratestackValueMap toWire() => {
        'limit': limit,
        'offset': offset,
        'fields': fields,
        'include': include,
        'sort': sort,
      };
}

class WidgetApi {
  const WidgetApi(this._runtime);

  final CratestackGrpcRuntime _runtime;

  Future<Page<Widget>> list([
    CratestackGrpcListInput input = const CratestackGrpcListInput(),
    CallOptions? options,
  ]) async {
    final result = await _runtime.unary(
      '/widgets_api.Api/ModelWidgetList',
      input.toWire(),
      _messages['WidgetRpcListInput']!,
      _messages['PageOfWidget']!,
      _messages,
      _enums,
      options: options,
    );
    return Page<Widget>.fromWire(
      result,
      decodeItem: (item) => Widget.fromWire(cratestackAsValueMap(item)),
    );
  }

  Future<Widget> get(int id, {CallOptions? options}) async {
    final result = await _runtime.unary(
      '/widgets_api.Api/ModelWidgetGet',
      {'id': id},
      _messages['WidgetRpcPkInput']!,
      _messages['Widget']!,
      _messages,
      _enums,
      options: options,
    );
    return Widget.fromWire(result);
  }

  Future<Widget> create(CreateWidgetInput input, {CallOptions? options}) async {
    final result = await _runtime.unary(
      '/widgets_api.Api/ModelWidgetCreate',
      input.toWire(),
      _messages['CreateWidgetInput']!,
      _messages['Widget']!,
      _messages,
      _enums,
      options: options,
    );
    return Widget.fromWire(result);
  }

  Future<Widget> update(
    int id,
    UpdateWidgetInput patch, {
    CallOptions? options,
  }) async {
    final result = await _runtime.unary(
      '/widgets_api.Api/ModelWidgetUpdate',
      {'id': id, 'patch': patch.toWire()},
      _messages['WidgetRpcUpdateInput']!,
      _messages['Widget']!,
      _messages,
      _enums,
      options: options,
    );
    return Widget.fromWire(result);
  }

  Future<void> delete(int id, {CallOptions? options}) async {
    await _runtime.unary(
      '/widgets_api.Api/ModelWidgetDelete',
      {'id': id},
      _messages['WidgetRpcPkInput']!,
      _messages['Widget']!,
      _messages,
      _enums,
      options: options,
    );
  }
}

