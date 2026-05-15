import 'package:tiny_rpc_client/tiny_rpc_client.dart';

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
}