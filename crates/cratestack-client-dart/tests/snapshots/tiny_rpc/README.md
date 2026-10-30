# tiny_rpc_client

Generated CrateStack Flutter package.

## Package Purpose

- typed CRUD APIs for generated CrateStack model routes
- typed procedure APIs for generated CrateStack procedure routes
- Riverpod providers for the generated client and APIs
- an HTTP transport seam (`CratestackHttpTransport`) that lets app code send through Dio or any other HTTP stack

This package does not own app-specific host selection, request signing policy, or transport interception policy. Those concerns stay with the host Flutter app and its HTTP stack.

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
- `lib/src/dio_transport.dart`
- `lib/src/queries.dart`
- `lib/src/constants.dart`
- `lib/src/models.dart`
- `lib/src/apis.dart`
- `example/main.dart`
- `test/tiny_rpc_client_test.dart`

Import only `package:tiny_rpc_client/tiny_rpc_client.dart` from application code. Files under `lib/src/` are implementation details.

## Adapter Setup

The generated client depends only on the `CratestackRpcAdapter` interface. The built-in implementation is `CratestackRpcHttpAdapter`, written once over a pure-Dart HTTP seam, `CratestackHttpTransport`. The adapter owns the headers, the CBOR/JSON choice, the response `Content-Type` dispatch and the error mapping; the transport owns the connection.

Dio is one transport (`CratestackDioTransport`, in `lib/src/dio_transport.dart`, the only file that names Dio):

```dart
final adapter = CratestackRpcHttpAdapter(
  CratestackDioTransport(myDio),
  baseUri: Uri.parse('https://api.example.com'),
);

final client = TinyRpcClientCratestackClient(
  adapter,
);
```

`baseUri` is joined with the request path (a path prefix such as `https://example.com/v1` is kept), so `myDio.options.baseUrl` is not used. Interceptors, base headers, certificate pinning and retry on `myDio` keep working.

Any other HTTP stack implements `CratestackHttpTransport` (one method, `send`) and is handed to the same adapter. Resolve with the response for every HTTP status; throw a `CratestackTransportException` (`Aborted`, `Unsent`, `TimedOut` or `Broken`) when there is no answer:

```dart
class MyTransport implements CratestackHttpTransport {
  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    // request.method, request.uri, request.headers, request.body (the exact
    // bytes to send), request.abort (completes when the caller gives up).
    throw UnimplementedError();
  }
}
```

Wire format:

- `format: CratestackWireFormat.cbor` (the default) writes CBOR bodies and prefers a CBOR answer
- `format: CratestackWireFormat.json` writes JSON bodies and prefers a JSON answer
- either way the answer is decoded by its own `Content-Type`

Errors:

- an HTTP error answer is a `CratestackRpcException` (`status`, `code`, `message`, `details`, `extra` and `challenges`)
- a refusal from the server's `AuthProvider` keeps its fixed code; the members it adds (`{"error": "attestation_expired"}`) are in `extra`, and its `WWW-Authenticate` value, raw, is in `challenges`
- a call that got no answer is a `CratestackTransportException`: `CratestackTransportUnsent` (provably never left the device, always safe to resend), `CratestackTransportAborted`, `CratestackTransportTimedOut` or `CratestackTransportBroken`. Never a Dio type.

Cancelling a call:

```dart
final cancel = Completer<void>();
final future = client.procedures.someCall(
  args,
  options: CratestackRpcCallOptions(abort: cancel.future),
);
cancel.complete(); // the call throws CratestackTransportAborted
```

Per-request headers:

```dart
const options = CratestackRpcCallOptions(
  headers: {'x-auth-id': '1'},
  idempotencyKey: 'order-42', // sent as Idempotency-Key
);
```

## Riverpod Setup

Generated providers include:

- `tinyRpcClientAdapterProvider`
- `tinyRpcClientClientProvider`
- `tinyRpcClientWidgetApiProvider`
- `tinyRpcClientProceduresApiProvider`

Typical overrides:

```dart
final container = ProviderContainer(
  overrides: [
    tinyRpcClientAdapterProvider.overrideWithValue(
CratestackRpcHttpAdapter(
        CratestackDioTransport(myDio),
        baseUri: Uri.parse('https://api.example.com'),
      ),
    ),
  ],
);
```

## Code Generation (`build_runner`)

Every generated data class carries `@CratestackBuilder(...)` (issue #668) — an annotation, not
working code, until `package:cratestack_builder` expands it into a `{Class}Builder` via
`build_runner`. Without this step the package will not compile, let alone
`flutter analyze` clean.

Run it after every regeneration:

```sh
dart run build_runner build --delete-conflicting-outputs
```

Or pass `--run-build-runner` to `cratestack generate-dart` to have the CLI run it for you right
after generation (opt-in; requires a Dart SDK on `PATH`):

```sh
cratestack generate-dart --schema schema.cstack --out . --run-build-runner
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

## Decimal Fields

A `Decimal`-typed schema field is generated as `package:decimal`'s `Decimal`
class (cratestack#498), not a `String`:

```dart
final invoice = await client.invoices.get(id);
print(invoice.amountXaf.toString()); // e.g. "0.0000001", never scientific notation
final total = invoice.amountXaf + invoice.taxXaf; // real arbitrary-precision arithmetic
```

**Migration from pre-#498 generated clients:** code that treated a `Decimal` field as a
`String` (string comparison, `double.parse`, concatenation) needs to switch to `Decimal`'s
own API — `Decimal.parse(input)` to construct one, `.toString()` to format, and its
comparison operators / `compareTo` for ordering. `DecimalFilter`'s `eq`/`ne`/`lt`/`lte`/
`gt`/`gte`/`in` fields need the same treatment when building a `Where`/`FindMany` argument
by hand.

This parses correctly regardless of which `Decimal` backend built the server
(`decimal-rust-decimal` or `decimal-bigdecimal` — see `cratestack-core`'s README): both
plain positional notation (`"0.0000001"`) and the scientific notation `bigdecimal` emits
past `rust_decimal`'s ~28-29 significant-digit capacity (`"1E-7"`) decode to the identical
value, and this package always re-encodes in plain notation.

## BigInt Fields

A `BigInt`-typed schema field is generated as `dart:core`'s `BigInt`, never `int`
(ADR 0019). It crosses the wire as a canonical decimal string (`"9223372036854775807"`),
because a JSON or CBOR number above 2^53 is rounded to a double on dart2js (Flutter web's
JavaScript build):

```dart
final account = await client.accounts.get(id);
print(account.balanceE8); // 9007199254740993, exact on the VM, dart2js and dart2wasm
final next = account.balanceE8! + BigInt.one; // real arbitrary-precision arithmetic
```

Decoding goes through `BigInt.parse`, which is exact on every Dart target; encoding uses
`toString()`. `BigIntFilter`'s `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`in` take `BigInt` values.
A number, or a string that is not canonical (`+5`, `007`, `-0`, whitespace), at a `BigInt`
key throws a `FormatException` naming the field, so a server from before the `BigInt`
cutover cannot hand this client a silently rounded value. A `BigInt` outside the `i64`
range is refused by the server, not by this client.

`Int` stays Dart's `int`; only fields declared `BigInt` in the schema change type.

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
- `sort` and `orderBy` must match if both are provided
- transport codec, envelope behavior, and signing belong to the Rust runtime bridge, not this package

generated files under `lib/src/` are implementation details; prefer importing the main library entry point only
