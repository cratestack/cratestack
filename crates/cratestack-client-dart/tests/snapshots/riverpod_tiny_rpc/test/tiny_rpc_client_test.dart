import 'package:flutter_test/flutter_test.dart';
import 'package:tiny_rpc_client/tiny_rpc_client.dart';
// Issue #331: the generated list provider's `input` parameter is
// `IMap<String, Object?>` (see `model_providers.dart.j2`'s own comment
// for why not a bare `Map`) — only imported here, not unconditionally
// above, since a paged first model (no `override_proof`) never
// exercises it and an unused import is a real `flutter analyze
// --fatal-warnings` failure.
import 'package:fast_immutable_collections/fast_immutable_collections.dart';
// `flutter_riverpod` itself is likewise only imported here — a schema
// with no models at all, or whose first model in schema order is paged
// (see `build_package_test.rs`'s own comment on why a paged first model
// gets no override-propagation proof), never emits the
// `ProviderContainer` tests below that are this import's only use.
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// A fake [CratestackRpcAdapter] the test below overrides
/// `tinyRpcClientAdapterProvider` with — the *existing*,
/// unmodified Dio override point (issue #302's acceptance criterion:
/// generated operation providers must be reachable through it, not
/// through a new one). Recording `calls` proves the generated
/// `WidgetApi.list()` method itself ran (not
/// bypassed), and returning a canned frame proves the decoded result
/// downstream came from *this* adapter, not a real network call.
class _FakeRpcAdapter implements CratestackRpcAdapter {
  final calls = <String>[];
  // Issue #331: proves the generated list provider's `input` actually
  // reaches the RPC call, not just that the opId routes correctly.
  final inputs = <Object?>[];

  @override
  Future<Object?> call(String opId, Object? input, {CratestackRpcCallOptions? options}) async {
    calls.add(opId);
    inputs.add(input);
    return <Object?>[<String, Object?>{}];
  }

  @override
  Future<List<RpcResponseFrame>> batch(List<RpcRequest> requests, {CratestackRpcCallOptions? options}) {
    throw UnimplementedError('unused by this override-propagation test');
  }

  @override
  Stream<Object?> stream(String opId, Object? input, {CratestackRpcCallOptions? options}) {
    throw UnimplementedError('unused by this override-propagation test');
  }
}

void main() {
  // A real `test()` case, not bare top-level `assert`s — the latter are
  // no-ops in a release-mode `flutter test` run, and a `test/` file with
  // no `test()` case at all when `override_proof` is unset (schemas with
  // no models, or whose first model in schema order is paged) is itself
  // a smell for a generated test scaffold. Wrapping this in `test(...)`
  // also gives `flutter_test` a real, unconditional use, independent of
  // whether the `ProviderContainer` tests below are emitted.
  test('rpc call options and generated surface', () {
    // `CratestackRpcCallOptions` (headers/idempotency key) is the RPC
    // transport's per-call options type — the REST transport's URL-query
    // builder types have no equivalent here. RPC calls carry a typed body
    // instead of a URL query, so there is no query-builder surface to
    // exercise in this package.
    const options = CratestackRpcCallOptions(
      headers: {'x-client': 'example'},
      idempotencyKey: 'example-key',
    );
    expect(options.idempotencyKey, 'example-key');
    expect(options.headers['x-client'], 'example');

    // Generated model API entry points:
    // - widgets

    // Generated procedures:
    // - echoName(...)

    // Round-trips the generated model class through the same
    // fromWire/toWire pair every RPC response and request body uses.
    final sample = Widget.fromWire(const <String, Object?>{});
    final wire = sample.toWire();
    expect(wire.containsKey('id'), isTrue);
  });

  test(
    'overriding tinyRpcClientAdapterProvider alone reaches widgetListProvider '
    '(issue #302: generated providers never construct their own adapter/client)',
    () async {
      final fakeAdapter = _FakeRpcAdapter();
      final container = ProviderContainer(
        overrides: [
          tinyRpcClientAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      // `widgetListProvider` now always takes an
      // optional `input` (issue #331), so `riverpod_generator` emits it
      // as a family — even the zero-argument default has to be called,
      // `widgetListProvider()`, not read bare.
      final result = await container.read(widgetListProvider().future);

      expect(fakeAdapter.calls, ['model.Widget.list']);
      expect(result, hasLength(1));
    },
  );

  test(
    'a non-default filter/pagination input passed to widgetListProvider reaches '
    'the underlying RPC call (issue #331: RPC has no typed query builder — see this story\'s PR body '
    'for that decision — but the untyped bag still has to actually reach the call, not just compile)',
    () async {
      final fakeAdapter = _FakeRpcAdapter();
      final container = ProviderContainer(
        overrides: [
          tinyRpcClientAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      final input = IMap<String, Object?>({'where': 'published=true', 'limit': 5});

      final result = await container.read(widgetListProvider(input: input).future);

      expect(fakeAdapter.calls, ['model.Widget.list']);
      expect(fakeAdapter.inputs.single, {'where': 'published=true', 'limit': 5});
      expect(result, hasLength(1));
    },
  );

  test(
    'widgetListProvider caches by input value, not identity '
    '(issue #331: a bare Map has identity-based ==, which is exactly the caching bug this story\'s '
    'REST fix addresses for CratestackListQuery — the RPC provider uses IMap instead specifically '
    'to avoid the same bug, verified here)',
    () async {
      final fakeAdapter = _FakeRpcAdapter();
      final container = ProviderContainer(
        overrides: [
          tinyRpcClientAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      // Two separately-constructed (non-identical) `IMap`s with the same
      // entries.
      final inputA = IMap<String, Object?>({'where': 'published=true'});
      final inputB = IMap<String, Object?>({'where': 'published=true'});
      expect(identical(inputA, inputB), isFalse);
      expect(inputA, equals(inputB));

      await container.read(widgetListProvider(input: inputA).future);
      await container.read(widgetListProvider(input: inputB).future);

      // A second read with a *value-equal* input must hit riverpod's
      // family cache, not fire a second RPC call.
      expect(fakeAdapter.calls, hasLength(1));
    },
  );
}
