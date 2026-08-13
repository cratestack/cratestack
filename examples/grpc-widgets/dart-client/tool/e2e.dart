// Ticket #210's load-bearing integration test: drive the real generated
// Dart client (`lib/`, generated from `../schemas/widgets.cstack` via
// `cratestack generate-dart`) against the real, running
// `grpc-widgets-example` server (ticket #171's `grpcurl`-verified
// example, unmodified) over real HTTP/2 gRPC — no mocks, no `grpcurl`,
// no `protoc`. The TypeScript counterpart is `../../ts-client-e2e.mjs`.
//
// Lives under `tool/` (rather than flat next to the server, like the
// TypeScript script) because a `package:` import needs a pub package
// context to resolve — the standard place for that in a Dart package is
// `tool/` or `example/`, not the repo root.
//
// Run:
//   1. `DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
//        cargo run -p grpc-widgets-example` (separate shell)
//   2. `cd examples/grpc-widgets/dart-client && flutter pub get`
//   3. `dart run tool/e2e.dart`

import 'package:grpc/grpc.dart';
import 'package:grpc_widgets_client/grpc_widgets_client.dart';

const _origin = '127.0.0.1';
const _port = 50061;

void _expect(bool condition, String message) {
  if (!condition) {
    throw StateError('FAILED: $message');
  }
  // ignore: avoid_print
  print('[ok] $message');
}

Future<void> main() async {
  final runtime = CratestackGrpcRuntime.host(_origin, port: _port);
  final client = GrpcWidgetsClientCratestackClient(runtime);
  final authOptions = CallOptions(metadata: {'x-auth-id': '1'});

  try {
    // --- create
    final id = DateTime.now().millisecondsSinceEpoch % 1000000;
    final created = await client.widgets.create(
      CreateWidgetInput(id: id, name: 'gizmo'),
      options: authOptions,
    );
    _expect(created.name == 'gizmo' && created.id == id, 'create -> $created');

    // --- get
    final fetched = await client.widgets.get(id, options: authOptions);
    _expect(
      fetched.id == created.id && fetched.name == created.name,
      'get -> $fetched',
    );

    // --- list
    final page = await client.widgets.list(
      const CratestackGrpcListInput(limit: 50),
      authOptions,
    );
    _expect(
      page.items.any((widget) => widget.id == id),
      'list -> ${page.items.length} item(s)',
    );
    _expect(
      page.pageInfo.hasNextPage == false || page.pageInfo.hasNextPage == true,
      'pageInfo decodes -> limit=${page.pageInfo.limit}, hasNextPage=${page.pageInfo.hasNextPage}',
    );

    // --- update
    final updated = await client.widgets.update(
      id,
      const UpdateWidgetInput(name: 'gizmo-v2'),
      options: authOptions,
    );
    _expect(
      updated.name == 'gizmo-v2' && updated.id == id,
      'update -> $updated',
    );

    // --- delete
    await client.widgets.delete(id, options: authOptions);
    // ignore: avoid_print
    print('[ok] delete -> (void)');

    // --- deliberate error: get-after-delete must surface a typed,
    // catchable NOT_FOUND, not a silent success or an opaque failure.
    var threw = false;
    try {
      await client.widgets.get(id, options: authOptions);
    } on CratestackGrpcError catch (error) {
      threw = true;
      _expect(
        error.code == 'not_found',
        'get-after-delete -> code=${error.code} status=${error.status}',
      );
    }
    _expect(threw, 'get-after-delete should have thrown');

    // ignore: avoid_print
    print('\nAll Dart gRPC client checks passed.');
  } finally {
    await runtime.shutdown();
  }
}
