import 'package:tiny_rest_client/tiny_rest_client.dart';

void main() {
  const fetchQuery = CratestackFetchQuery();
  final selection = WidgetSelection();
  selection.id();
  final listQuery = selection.toListQuery(
    sort: '-id',
    limit: 20,
    offset: 0,
    where: 'published=true',
  );
  final projection = selection.asProjection();

  // Generated model API entry points:
  // - widgets

  // Generated procedures:
  // - echoName(...)

  assert(listQuery.limit == 20);
  assert(fetchQuery.toQueryParameters().isEmpty);
  assert(selection.toFetchQuery().toQueryParameters().isNotEmpty);
  assert(projection.toFetchQuery().toQueryParameters().isNotEmpty);
}