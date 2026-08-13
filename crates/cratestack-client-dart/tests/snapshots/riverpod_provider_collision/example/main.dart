import 'package:dart_verify_riverpod_collision/dart_verify_riverpod_collision.dart';

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
  // - widgetLists

  // Generated procedures:
  // - widgetCreate(...)

  assert(listQuery.limit == 20);
  assert(fetchQuery.toQueryParameters().isEmpty);
  assert(selection.toFetchQuery().toQueryParameters().isNotEmpty);
  assert(projection.toFetchQuery().toQueryParameters().isNotEmpty);
}
