# grpc_widgets_client

Generated CrateStack gRPC client package (`transport grpc`).

## Package Purpose

- typed CRUD APIs (`list`/`get`/`create`/`update`/`delete`, `create` only where the schema's policy allows it) for every model the schema exposes over gRPC
- a hand-rolled protobuf wire codec (varint/fixed64/length-delimited, `google.protobuf.Timestamp`, enum tables) — no `protoc`/`protoc-gen-dart` step, no protobuf-runtime dependency
- real HTTP/2 gRPC transport via [`package:grpc`](https://pub.dev/packages/grpc)'s `ClientChannel`/`ClientMethod`

Procedures and server-streaming are out of scope: `cratestack-grpc`'s generated tonic service doesn't expose them, so there is nothing for this client to call. `where`/`or`/structured `filters`/`includeFields` are also not wired into `list()` — only `limit`/`offset`/`fields`/`include`/`sort`, the same reduced set the TypeScript gRPC-Web client (ticket #172) ships.

## Install And Import

```yaml
dependencies:
  grpc_widgets_client:
    path: ../dart-client
```

Main import:

```dart
import 'package:grpc_widgets_client/grpc_widgets_client.dart';
```

## Project Layout

- `pubspec.yaml`
- `README.md`
- `CHANGELOG.md`
- `analysis_options.yaml`
- `lib/grpc_widgets_client.dart`
- `lib/src/runtime.dart` — the protobuf codec, `CratestackGrpcRuntime`, `CratestackGrpcError`
- `lib/src/models.dart` — model/`Create`/`Update` data classes, enums, `Page`/`PageInfo`
- `lib/src/apis.dart` — the generated client class and per-model API classes

Import only `package:grpc_widgets_client/grpc_widgets_client.dart` from application code. Files under `lib/src/` are implementation details.

## Connecting

The server this ticket targets is plaintext HTTP/2 (h2c) — `ChannelCredentials.insecure()` matches that, and is the default the convenience constructor below uses.

```dart
final runtime = CratestackGrpcRuntime.host('127.0.0.1', port: 50061);
final client = GrpcWidgetsClientCratestackClient(runtime);
```

Against a TLS-terminated endpoint, build the channel yourself and pass it in:

```dart
final runtime = CratestackGrpcRuntime(
  ClientChannel(
    'grpc.example.com',
    port: 443,
    options: const ChannelOptions(credentials: ChannelCredentials.secure()),
  ),
);
final client = GrpcWidgetsClientCratestackClient(runtime);
```

## CRUD Usage

```dart
final widgets = client.widgets;
final page = await widgets.list();
final item = await widgets.get(page.items.first.id);
final created = await widgets.create(CreateWidgetInput(/* ... */));
final updated = await widgets.update(item.id, UpdateWidgetInput(/* ... */));
await widgets.delete(item.id);
```

`list()` takes an optional `CratestackGrpcListInput(limit: ..., offset: ..., fields: [...], include: [...], sort: ...)`. Every method also takes an optional `CallOptions? options` (from `package:grpc`) for per-call metadata (auth headers, deadlines) — pass it instead of baking auth into the runtime if it can change between calls:

```dart
final options = CallOptions(metadata: {'authorization': 'Bearer $token'});
final page = await widgets.list(const CratestackGrpcListInput(), options);
final item = await widgets.get(id, options: options);
```

## Closing The Connection

`CratestackGrpcRuntime` opens a real HTTP/2 connection (via its `channel` field) that stays open until you close it — a short-lived process (a script, a CLI tool, a test) will hang on exit otherwise:

```dart
await runtime.shutdown(); // graceful: lets in-flight calls finish
// or: await runtime.terminate(); // immediate
```

## Errors

A failed call throws `CratestackGrpcError`, wrapping `package:grpc`'s `GrpcError` with a stable, friendly `code` string alongside the raw numeric gRPC `status`:

```dart
try {
  await someModel.get(id);
} on CratestackGrpcError catch (error) {
  if (error.code == 'not_found') {
    // ...
  }
}
```

`code` mirrors the TypeScript gRPC-Web client's mapping table, so both generated clients report the same string for the same server-side error.

## Limitations

- model CRUD only — no procedures, no server-streaming
- `list()` supports `limit`/`offset`/`fields`/`include`/`sort`; raw predicate queries and structured filters are not wired up yet
- no schema-fingerprint (`x-cratestack-schema-sha`) header on gRPC calls yet — REST and RPC only for now
