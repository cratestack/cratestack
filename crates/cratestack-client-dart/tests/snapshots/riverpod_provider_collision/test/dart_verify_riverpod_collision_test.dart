import 'package:flutter_test/flutter_test.dart';
import 'package:dart_verify_riverpod_collision/dart_verify_riverpod_collision.dart';
// Only imported when `override_proof` is set — a schema with no models
// at all, or whose first model in schema order is paged (see
// `build_package_test.rs`'s own comment on why a paged first model gets
// no override-propagation proof), never emits the `ProviderContainer`
// tests below that are this import's only use. An unconditional import
// here used to make `flutter analyze --fatal-warnings` (which the
// generated `analysis_options.yaml` enables via `flutter_lints/flutter.yaml`)
// fail on `unused_import` for exactly that shape of schema.
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// A fake [CratestackClientAdapter] the test below overrides
/// `dartVerifyRiverpodCollisionAdapterProvider` with — the *existing*,
/// unmodified Dio override point (issue #302's acceptance criterion:
/// generated operation providers must be reachable through it, not
/// through a new one). Recording `requests` proves the generated
/// `WidgetApi.list()` method itself ran (not
/// bypassed), and returning a canned body proves the decoded result
/// downstream came from *this* adapter, not a real network call.
class _FakeClientAdapter implements CratestackClientAdapter {
  final requests = <CratestackRequest>[];

  @override
  Future<Object?> execute(CratestackRequest request, {CratestackCallOptions? options}) async {
    requests.add(request);
    return <Object?>[<String, Object?>{}];
  }
}

void main() {
  // A real `test()` case, not bare top-level `assert`s — the latter are
  // no-ops in a release-mode `flutter test` run (asserts only fire under
  // `--enable-asserts`, on by default for `flutter test` but not
  // guaranteed by every runner), and a `test/` file that contains no
  // `test()` case at all when `override_proof` is unset (schemas with no
  // models, or whose first model in schema order is paged) is itself a
  // smell for a generated test scaffold. Wrapping this in `test(...)`
  // also gives `flutter_test` a real, unconditional use, independent of
  // whether the `ProviderContainer` tests below are emitted.
  test('query parameters', () {
    const fetchQuery = CratestackFetchQuery();
    final selection = WidgetSelection();
    selection.id();
    final listQuery = selection.toListQuery(
      sort: '-id',
      limit: 20,
      offset: 0,
      where: 'published=true',
      orFilters: ['published=true', 'published=false'],
      filters: {'status': 'active'},
    );
    final projection = selection.asProjection();

    expect(listQuery.toQueryParameters()['sort'], '-id');
    expect(listQuery.toQueryParameters()['limit'], 20);
    expect(listQuery.toQueryParameters()['offset'], 0);
    expect(listQuery.toQueryParameters()['where'], 'published=true');
    expect(listQuery.toQueryParameters()['or'], 'published=true|published=false');
    expect(listQuery.toQueryParameters()['status'], 'active');
    expect(fetchQuery.toQueryParameters(), isEmpty);
    expect(listQuery.toQueryParameters()['fields'], isNotNull);
    expect(selection.toFetchQuery().toQueryParameters(), isNotEmpty);
    expect(projection.toFetchQuery().toQueryParameters(), isNotEmpty);
  });

  test(
    'overriding dartVerifyRiverpodCollisionAdapterProvider alone reaches widgetListProvider '
    '(issue #302: generated providers never construct their own adapter/client)',
    () async {
      final fakeAdapter = _FakeClientAdapter();
      final container = ProviderContainer(
        overrides: [
          dartVerifyRiverpodCollisionAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      // `widgetListProvider` now always takes an
      // optional `query` (issue #331), so `riverpod_generator` emits it
      // as a family — even the zero-argument default has to be called,
      // `widgetListProvider()`, not read bare.
      final result = await container.read(widgetListProvider().future);

      expect(fakeAdapter.requests, hasLength(1));
      expect(result, hasLength(1));
    },
  );

  test(
    'a non-default CratestackListQuery passed to widgetListProvider reaches the '
    'underlying HTTP call (issue #331: the generated list provider forwards its query argument, '
    'not just the zero-argument default)',
    () async {
      final fakeAdapter = _FakeClientAdapter();
      final container = ProviderContainer(
        overrides: [
          dartVerifyRiverpodCollisionAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      // Deliberately not `const` — a `const` literal would canonicalize
      // to a single shared instance regardless of whether
      // `CratestackListQuery` has real value equality, which would let
      // this test pass even without issue #331's `operator ==`/
      // `hashCode` fix. Constructing it fresh here is what actually
      // exercises that fix.
      final query = CratestackListQuery(where: 'published=true', limit: 5);

      final result = await container.read(widgetListProvider(query: query).future);

      expect(fakeAdapter.requests, hasLength(1));
      expect(fakeAdapter.requests.single.queryParameters?['where'], 'published=true');
      expect(fakeAdapter.requests.single.queryParameters?['limit'], 5);
      expect(result, hasLength(1));
    },
  );

  test(
    'widgetListProvider caches by query value, not identity '
    '(issue #331: without CratestackListQuery.operator==/hashCode, a freshly-constructed-but-equal '
    'query never hits riverpod\'s family cache and the provider re-fetches on every rebuild — the '
    'exact bug class issue #325 already fixed for generated data classes)',
    () async {
      final fakeAdapter = _FakeClientAdapter();
      final container = ProviderContainer(
        overrides: [
          dartVerifyRiverpodCollisionAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      // Two separately-constructed (non-const, non-identical) instances
      // with the same field values.
      final queryA = CratestackListQuery(where: 'published=true');
      final queryB = CratestackListQuery(where: 'published=true');
      expect(identical(queryA, queryB), isFalse);
      expect(queryA, equals(queryB));

      await container.read(widgetListProvider(query: queryA).future);
      await container.read(widgetListProvider(query: queryB).future);

      // A second read with a *value-equal* query must hit riverpod's
      // family cache, not fire a second HTTP call.
      expect(fakeAdapter.requests, hasLength(1));
    },
  );
}
