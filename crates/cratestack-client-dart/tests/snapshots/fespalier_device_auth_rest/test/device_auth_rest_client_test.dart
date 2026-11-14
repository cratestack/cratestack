import 'package:flutter_test/flutter_test.dart';
import 'package:device_auth_rest_client/device_auth_rest_client.dart';

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
    const listQuery = CratestackListQuery(
      sort: '-id',
      limit: 20,
      offset: 0,
      where: 'published=true',
      orFilters: ['published=true', 'published=false'],
      filters: {'status': 'active'},
    );

    expect(listQuery.toQueryParameters()['sort'], '-id');
    expect(listQuery.toQueryParameters()['limit'], 20);
    expect(listQuery.toQueryParameters()['offset'], 0);
    expect(listQuery.toQueryParameters()['where'], 'published=true');
    expect(listQuery.toQueryParameters()['or'], 'published=true|published=false');
    expect(listQuery.toQueryParameters()['status'], 'active');
    expect(fetchQuery.toQueryParameters(), isEmpty);
  });

}
