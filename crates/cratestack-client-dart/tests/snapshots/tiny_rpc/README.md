# tiny_rpc_client

Generated CrateStack Flutter package.

## Package Purpose

- typed CRUD APIs for generated CrateStack model routes
- typed procedure APIs for generated CrateStack procedure routes
- Riverpod providers for the generated client and APIs
- an adapter seam that lets app code provide Dio or another transport implementation

This package does not own app-specific host selection, request signing policy, or transport interception policy. Those concerns stay with the host Flutter app and its Dio stack.

## Install And Import

This generated package is Flutter-shaped because it includes Riverpod providers.

```yaml
dependencies:
  tiny_rpc_client:
    path: ../tiny_rpc_client
```

Main import:

```dart
import 'package:tiny_rpc_client/tiny_rpc_client.dart';
```

## Project Layout

- `pubspec.yaml`
- `README.md`
- `CHANGELOG.md`
- `analysis_options.yaml`
- `lib/tiny_rpc_client.dart`
- `lib/src/runtime.dart`
- `lib/src/queries.dart`
- `lib/src/constants.dart`
- `lib/src/models.dart`
- `lib/src/apis.dart`
- `example/main.dart`
- `test/tiny_rpc_client_test.dart`

Import only `package:tiny_rpc_client/tiny_rpc_client.dart` from application code. Files under `lib/src/` are implementation details.

## Adapter Setup

The generated package expects a transport adapter implementation. The built-in default is `CratestackDioAdapter`, but the client only depends on the `CratestackClientAdapter` interface.

Minimal shape:

```dart
class MyAdapter implements CratestackClientAdapter {
  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    throw UnimplementedError();
  }
}
```

Client construction:

```dart
final client = TinyRpcClientCratestackClient(
  myAdapter,
  basePath: '/api',
);
```

Dio-backed setup:

```dart
final adapter = CratestackDioAdapter(
  dio: myDio,
  useRustTransport: true,
);

final client = TinyRpcClientCratestackClient(
  adapter,
  basePath: '/api',
);
```

CBOR-over-Dio setup for web or non-Rust platforms:

```dart
final adapter = CratestackCborDioAdapter(dio: myDio);

final client = TinyRpcClientCratestackClient(
  adapter,
  basePath: '/api',
);
```

Rule of thumb:

- use `CratestackDioAdapter(dio: ..., useRustTransport: true)` when Rust should execute the request
- use `CratestackDioAdapter(dio: ..., useRustTransport: false)` for plain JSON/Dio flows
- use `CratestackCborDioAdapter(dio: ...)` when the host should stay on Dio/browser transport but speak CBOR on the wire

Per-request headers:

```dart
const options = CratestackCallOptions(
  headers: {'x-auth-id': '1'},
);
```

## Riverpod Setup

Generated providers include:

- `tinyRpcClientAdapterProvider`
- `tinyRpcClientBasePathProvider`
- `tinyRpcClientClientProvider`
- `tinyRpcClientWidgetApiProvider`
- `tinyRpcClientProceduresApiProvider`

Typical overrides:

```dart
final container = ProviderContainer(
  overrides: [
    tinyRpcClientAdapterProvider.overrideWithValue(
      CratestackDioAdapter(dio: myDio, useRustTransport: true),
    ),
    tinyRpcClientBasePathProvider.overrideWith((ref) => '/api'),
  ],
);
```

## Flutter Usage

Selection-shaped list query in Flutter or Riverpod:

```dart
final tinyRpcClientWidgetListProvider = FutureProvider((ref) async {
  final client = ref.watch(tinyRpcClientClientProvider);
  final selection = WidgetSelection()
    ..id()
;

  return client.widgets.list(
    query: selection.toListQuery(
      sort: '-id',
      limit: 20,
      where: 'published=true',
    ),
  );
});
```

Projection-backed detail query in Flutter or Riverpod:

```dart
final tinyRpcClientWidgetCardProvider = FutureProvider.family((ref, int id) async {
  final client = ref.watch(tinyRpcClientClientProvider);
  final selection = WidgetSelection()
    ..id()
;

  return client.widgets.getView(
    id,
    projection: selection.asProjection(),
  );
});
```

Rule of thumb:

- use `selection.toListQuery(...)` when the screen still wants full generated model types back from `list(...)` or `get(...)`
- use `selection.asProjection()` with `getView(...)` or `listView(...)` when the screen only needs a shaped payload and should receive a projected wrapper type

## Paged Models

When a model opts into `@@paged`, generated Dart list APIs switch from `List<T>` to `Page<T>` for that model.

Full-model paged Flutter example:

```dart
final tinyRpcClientWidgetPagedProvider = FutureProvider((ref) async {
  final client = ref.watch(tinyRpcClientClientProvider);
  final selection = WidgetSelection()
    ..id()
;

  return client.widgets.list(
    query: selection.toListQuery(
      sort: '-id',
      limit: 20,
      offset: 0,
      where: 'published=true',
    ),
  );
});

Widget buildPagedList(WidgetRef ref) {
  final page = ref.watch(tinyRpcClientWidgetPagedProvider);

  return page.when(
    data: (page) => ListView.builder(
      itemCount: page.items.length,
      itemBuilder: (context, index) {
        final item = page.items[index];
        return ListTile(title: Text(item.id.toString()));
      },
    ),
    loading: () => const CircularProgressIndicator(),
    error: (error, _) => Text('$error'),
  );
}
```

Projected paged Flutter example:

```dart
final tinyRpcClientWidgetProjectedPageProvider = FutureProvider((ref) async {
  final client = ref.watch(tinyRpcClientClientProvider);
  final selection = WidgetSelection()
    ..id()
;

  return client.widgets.listView(
    projection: selection.asProjection(),
    query: const CratestackListQuery(
      limit: 20,
      offset: 0,
      sort: '-id',
      where: 'published=true',
    ),
  );
});

String describePage(Page<ProjectedWidget> page) {
  final count = page.items.length;
  final total = page.totalCount;
  final hasNext = page.pageInfo.hasNextPage;
  return 'items=$count total=$total hasNext=$hasNext';
}
```

Flutter UI example using those providers:

```dart
class WidgetPagedScreen extends ConsumerWidget {
  const WidgetPagedScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final page = ref.watch(tinyRpcClientWidgetPagedProvider);

    return Scaffold(
      appBar: AppBar(title: const Text('Widgets')),
      body: page.when(
        data: (page) => Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text('Total: ${page.totalCount ?? page.items.length}'),
            ),
            Expanded(
              child: ListView.builder(
                itemCount: page.items.length,
                itemBuilder: (context, index) {
                  final item = page.items[index];
                  return ListTile(
                    title: Text(item.id.toString()),
                    subtitle: Text('hasNextPage=${page.pageInfo.hasNextPage}'),
                  );
                },
              ),
            ),
          ],
        ),
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (error, _) => Center(child: Text('$error')),
      ),
    );
  }
}

class WidgetCardView extends ConsumerWidget {
  const WidgetCardView({super.key, required this.id});

  final int id;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final item = ref.watch(tinyRpcClientWidgetCardProvider(id));

    return item.when(
      data: (item) => Card(
        child: ListTile(
          title: Text(item.id?.toString() ?? ''),
        ),
      ),
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => Center(child: Text('$error')),
    );
  }
}
```

For `@@paged` models:

- `list(...)` returns `Page<Model>`
- `listView(...)` returns `Page<ProjectedModel>`
- the paging envelope stays stable at `items`, `totalCount`, and `pageInfo`
- only the item type changes between full-model and projected flows

## CRUD Usage

```dart
final listSelection = WidgetSelection();
listSelection.id();

final listQuery = listSelection.toListQuery(
  sort: '-id',
  limit: 20,
  offset: 0,
  where: 'published=true',
  orFilters: ['published=true', 'published=false'],
  filters: {'status': 'active'},
);

final items = await client.widgets.list(
  query: listQuery,
  options: options,
);

final item = await client.widgets.get(
  1,
  query: const CratestackFetchQuery(),
  options: options,
);
```

Generated model entry points:

- `client.widgets`

## Procedure Usage

All generated procedures use `POST` at the HTTP layer, but the package exposes them as typed Dart methods.

Query procedures:

- `client.procedures.echoName(EchoNameArgs(...), options: options)`

Mutation procedures:

- none in this schema

Full generated procedure inventory:

- `query` `client.procedures.echoName(...)` returns `String`

## Query Parameters

The generated query helpers cover the canonical client-side query contract:

- `fields`
- `include`
- `includeFields[path]`
- `sort`
- `orderBy`
- `limit`
- `offset`
- `where`
- legacy `or`
- resource-specific filters via `filters`

Example:

```dart
final params = (WidgetSelection()
  ..id()
).toListQuery(
  sort: '-id',
  limit: 20,
  offset: 0,
  where: 'published=true',
  orFilters: ['published=true', 'published=false'],
  filters: {'status': 'active'},
).toQueryParameters();
```

## Generated Constants

Use the generated constant groups to avoid stringly-typed `fields` and `include` selections.

- `WidgetFieldNames`
- `WidgetIncludeNames`

Example:

```dart
final fields = <String>[WidgetFieldNames.id];
final include = <String>[];
```

## Projection Views

Generated selection builders can still express projected reads without hand-writing `fields`, `include`, and `includeFields[path]` strings.

```dart
final selection = WidgetSelection();
selection.id();
final projection = selection.asProjection();

final item = await client.widgets.getView(
  1,
  projection: projection,
  options: options,
);

final items = await client.widgets.listView(
  projection: projection,
  query: const CratestackListQuery(limit: 20),
  options: options,
);
```

Projection views return `T` for `getView(...)` and `List<T>` or `Page<T>` for `listView(...)`, depending on whether the model list route is paged.

Use `selection.toListQuery(...)` when you want the plain `list(...)` or `get(...)` APIs but do not want to hand-write `fields`, `include`, and `includeFields[path]`. Use `selection.asProjection()` with `getView(...)` or `listView(...)` when you want projected wrapper types back.

Why this speeds development:

```dart
// Before
final posts = await client.widgets.list(
  query: const CratestackListQuery(
    fields: [WidgetFieldNames.id],
    sort: '-id',
    limit: 20,
    where: 'published=true',
  ),
);

// After
final selection = WidgetSelection()
  ..id()
;

final posts = await client.widgets.list(
  query: selection.toListQuery(
    sort: '-id',
    limit: 20,
    where: 'published=true',
  ),
);
```

This cuts down on repeated query-shape plumbing, keeps nested relation shape next to the relation itself, and makes screen/query iteration faster because adding one field usually means adding one builder call.

The generated field/include constants are still useful. Keep them for low-level dynamic query composition, persisted query settings, user-driven field pickers, or any code path that cannot rely on one static generated selection builder.

## Generated APIs

Model entry points:

- `client.widgets`

Procedure entry points:

- `client.procedures.echoName(...)`

## Bridge Contract

The Dart side sends:

- method
- path
- canonical query string
- headers
- body bytes

The Rust bridge is expected to:

- interpret bridge bytes
- transcode to the configured transport codec
- execute HTTP
- decode the HTTP response
- return bridge bytes to Dart

Bridge bytes are an internal runtime format, not the public transport codec contract.

## Limitations

- model fields are generated as projection-safe nullable properties so `fields`, `include`, and `includeFields[path]` remain usable
- the generated Dart package still relies on generic value-graph conversion for typed model shaping
- the bridge currently uses JSON bytes internally even when the HTTP transport codec is CBOR
- the generated Dart APIs do not yet surface a first-class typed remote-error API
- `sort` and `orderBy` must match if both are provided
- transport codec, envelope behavior, and signing belong to the Rust runtime bridge, not this package

generated files under `lib/src/` are implementation details; prefer importing the main library entry point only
