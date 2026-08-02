import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:dart_verify_riverpod_collision/dart_verify_riverpod_collision.dart';

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

  assert(listQuery.toQueryParameters()['sort'] == '-id');
  assert(listQuery.toQueryParameters()['limit'] == 20);
  assert(listQuery.toQueryParameters()['offset'] == 0);
  assert(listQuery.toQueryParameters()['where'] == 'published=true');
  assert(listQuery.toQueryParameters()['or'] == 'published=true|published=false');
  assert(listQuery.toQueryParameters()['status'] == 'active');
  assert(fetchQuery.toQueryParameters().isEmpty);
  assert(listQuery.toQueryParameters()['fields'] != null);
  assert(selection.toFetchQuery().toQueryParameters().isNotEmpty);
  assert(projection.toFetchQuery().toQueryParameters().isNotEmpty);

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

      final result = await container.read(widgetListProvider.future);

      expect(fakeAdapter.requests, hasLength(1));
      expect(result, hasLength(1));
    },
  );
}
