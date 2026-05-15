import 'models.dart';
import 'runtime.dart';

abstract interface class CratestackProjection<T> {
  CratestackFetchQuery toFetchQuery();
  T fromWire(CratestackValueMap value);
}

class CratestackSelectionProjection<T> implements CratestackProjection<T> {
  const CratestackSelectionProjection({
    required this.query,
    required this.decode,
  });

  final CratestackFetchQuery query;
  final T Function(CratestackValueMap value) decode;

  @override
  CratestackFetchQuery toFetchQuery() => query;

  @override
  T fromWire(CratestackValueMap value) => decode(value);
}

class CratestackSelectionNode {
  CratestackSelectionNode();

  final Set<String> fields = <String>{};
  final Map<String, CratestackSelectionNode> includes = <String, CratestackSelectionNode>{};
}

CratestackFetchQuery cratestackSelectionToFetchQuery(CratestackSelectionNode node) {
  final fields = node.fields.toList(growable: false);
  final include = <String>[];
  final includeFields = <String, List<String>>{};

  void visit(String prefix, CratestackSelectionNode current) {
    for (final entry in current.includes.entries) {
      final path = prefix.isEmpty ? entry.key : '$prefix.${entry.key}';
      include.add(path);
      if (entry.value.fields.isNotEmpty) {
        includeFields[path] = entry.value.fields.toList(growable: false);
      }
      visit(path, entry.value);
    }
  }

  visit('', node);
  return CratestackFetchQuery(
    fields: fields,
    include: include,
    includeFields: includeFields,
  );
}

CratestackListQuery cratestackMergeSelectionIntoListQuery(
  CratestackListQuery? query,
  CratestackSelectionNode node,
) {
  return cratestackMergeFetchIntoListQuery(query, cratestackSelectionToFetchQuery(node));
}

CratestackListQuery cratestackMergeFetchIntoListQuery(
  CratestackListQuery? query,
  CratestackFetchQuery fetchQuery,
) {
  final base = query ?? const CratestackListQuery();
  if (base.fields.isNotEmpty || base.include.isNotEmpty || base.includeFields.isNotEmpty) {
    throw ArgumentError(
      'projections own fields/include/includeFields; leave them empty on CratestackListQuery',
    );
  }
  return CratestackListQuery(
    fields: fetchQuery.fields,
    include: fetchQuery.include,
    includeFields: fetchQuery.includeFields,
    limit: base.limit,
    offset: base.offset,
    sort: base.sort,
    orderBy: base.orderBy,
    where: base.where,
    orFilters: base.orFilters,
    filters: base.filters,
  );
}

class CratestackListQuery {
  const CratestackListQuery({
    this.fields = const <String>[],
    this.include = const <String>[],
    this.includeFields = const <String, List<String>>{},
    this.limit,
    this.offset,
    this.sort,
    this.orderBy,
    this.where,
    this.orFilters = const <String>[],
    this.filters = const <String, String>{},
  });

  /// Scalar fields to keep on the primary resource payload.
  final List<String> fields;
  /// Declared relation paths to embed in the response.
  final List<String> include;
  /// Scalar fields to keep on each included relation payload.
  final Map<String, List<String>> includeFields;
  final int? limit;
  final int? offset;
  final String? sort;
  final String? orderBy;
  final String? where;
  final List<String> orFilters;
  final Map<String, String> filters;

  Map<String, Object?> toQueryParameters() {
    final query = <String, Object?>{}..addAll(filters);
    if (fields.isNotEmpty) query['fields'] = fields.join(',');
    if (include.isNotEmpty) query['include'] = include.join(',');
    for (final entry in includeFields.entries) {
      if (entry.value.isNotEmpty) {
        query['includeFields[${entry.key}]'] = entry.value.join(',');
      }
    }
    if (limit != null) query['limit'] = limit;
    if (offset != null) query['offset'] = offset;
    final effectiveSort = sort ?? orderBy;
    if (effectiveSort != null && effectiveSort.isNotEmpty) query['sort'] = effectiveSort;
    if (sort != null && orderBy != null && sort != orderBy) {
      throw ArgumentError('sort and orderBy must match when both are provided');
    }
    if (where != null && where!.isNotEmpty) query['where'] = where;
    if (orFilters.isNotEmpty) query['or'] = orFilters.join('|');
    return query;
  }
}

class WidgetSelection {
  WidgetSelection();

  final CratestackSelectionNode _node = CratestackSelectionNode();

  WidgetSelection id() {
    _node.fields.add('id');
    return this;
  }

  WidgetSelection name() {
    _node.fields.add('name');
    return this;
  }

  WidgetSelection weight() {
    _node.fields.add('weight');
    return this;
  }

  CratestackFetchQuery toFetchQuery() => cratestackSelectionToFetchQuery(_node);

  CratestackListQuery toListQuery({
    int? limit,
    int? offset,
    String? sort,
    String? orderBy,
    String? where,
    List<String> orFilters = const <String>[],
    Map<String, String> filters = const <String, String>{},
  }) {
    final fetchQuery = toFetchQuery();
    return CratestackListQuery(
      fields: fetchQuery.fields,
      include: fetchQuery.include,
      includeFields: fetchQuery.includeFields,
      limit: limit,
      offset: offset,
      sort: sort,
      orderBy: orderBy,
      where: where,
      orFilters: orFilters,
      filters: filters,
    );
  }

  CratestackProjection<ProjectedWidget> asProjection() {
    return CratestackSelectionProjection<ProjectedWidget>(
      query: toFetchQuery(),
      decode: (value) => ProjectedWidget.fromWire(value),
    );
  }
}

class WidgetIncludeSelection {
  WidgetIncludeSelection();

  final CratestackSelectionNode _node = CratestackSelectionNode();

  WidgetIncludeSelection id() {
    _node.fields.add('id');
    return this;
  }

  WidgetIncludeSelection name() {
    _node.fields.add('name');
    return this;
  }

  WidgetIncludeSelection weight() {
    _node.fields.add('weight');
    return this;
  }

}


class CratestackFetchQuery {
  const CratestackFetchQuery({
    this.fields = const <String>[],
    this.include = const <String>[],
    this.includeFields = const <String, List<String>>{},
  });

  /// Scalar fields to keep on the primary resource payload.
  final List<String> fields;
  /// Declared relation paths to embed in the response.
  final List<String> include;
  /// Scalar fields to keep on each included relation payload.
  final Map<String, List<String>> includeFields;

  Map<String, Object?> toQueryParameters() {
    final query = <String, Object?>{};
    if (fields.isNotEmpty) query['fields'] = fields.join(',');
    if (include.isNotEmpty) query['include'] = include.join(',');
    for (final entry in includeFields.entries) {
      if (entry.value.isNotEmpty) {
        query['includeFields[${entry.key}]'] = entry.value.join(',');
      }
    }
    return query;
  }
}