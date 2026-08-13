import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';

void main() {
  const fetchQuery = CratestackFetchQuery();
  final selection = BoardSelection();
  selection.id();
  final listQuery = selection.toListQuery(
    sort: '-id',
    limit: 20,
    offset: 0,
    where: 'published=true',
  );
  final projection = selection.asProjection();

  // Generated model API entry points:
  // - boards
  // - tasks

  // Generated procedures:
  // - estimateFocusMinutes(...)

  assert(listQuery.limit == 20);
  assert(fetchQuery.toQueryParameters().isEmpty);
  assert(selection.toFetchQuery().toQueryParameters().isNotEmpty);
  assert(projection.toFetchQuery().toQueryParameters().isNotEmpty);
}
