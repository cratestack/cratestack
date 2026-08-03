// `Page`/`PageInfo` (every `@@paged` model's `list()` return type) plus
// any type/enum referenced by more than one model, imported by every
// file that needs them rather than duplicated — see the ownership rule
// documented at `crate::riverpod::partition::Owner`.
import 'dart:typed_data';

import 'package:fast_immutable_collections/fast_immutable_collections.dart';

import '../runtime.dart';


class PageInfo {
  const PageInfo({
    this.limit,
    this.offset,
    required this.hasNextPage,
    required this.hasPreviousPage,
  });

  final int? limit;
  final int? offset;
  final bool hasNextPage;
  final bool hasPreviousPage;

  factory PageInfo.fromWire(CratestackValueMap value) {
    return PageInfo(
      limit: value['limit'] == null ? null : (value['limit'] as num).toInt(),
      offset: value['offset'] == null ? null : (value['offset'] as num).toInt(),
      hasNextPage: cratestackRequireWireValue('PageInfo', 'hasNextPage', value['hasNextPage']) as bool,
      hasPreviousPage: cratestackRequireWireValue('PageInfo', 'hasPreviousPage', value['hasPreviousPage']) as bool,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'limit': limit,
      'offset': offset,
      'hasNextPage': hasNextPage,
      'hasPreviousPage': hasPreviousPage,
    };
  }
}

class Page<T> {
  const Page({
    required this.items,
    this.totalCount,
    required this.pageInfo,
  });

  final IList<T> items;
  final int? totalCount;
  final PageInfo pageInfo;

  factory Page.fromWire(
    CratestackValueMap value, {
    required T Function(Object? item) decodeItem,
  }) {
    return Page<T>(
      items: cratestackAsValueList(cratestackRequireWireValue('Page', 'items', value['items']))
          .map((item) => decodeItem(item))
          .toIList(),
      totalCount: value['totalCount'] == null ? null : (value['totalCount'] as num).toInt(),
      pageInfo: PageInfo.fromWire(
        cratestackAsValueMap(cratestackRequireWireValue('Page', 'pageInfo', value['pageInfo'])),
      ),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'items': items.map((item) {
        if (item is DateTime) return item.toUtc().toIso8601String();
        if (item is Uint8List) return item.toList(growable: false);
        if (item is String || item is num || item is bool || item == null) return item;
        return (item as dynamic).toWire();
      }).toList(growable: false),
      'totalCount': totalCount,
      'pageInfo': pageInfo.toWire(),
    };
  }
}

class PageInput {
  const PageInput({this.limit, this.offset});

  final int? limit;
  final int? offset;

  factory PageInput.fromWire(CratestackValueMap value) {
    return PageInput(
      limit: value['limit'] == null ? null : (value['limit'] as num).toInt(),
      offset: value['offset'] == null ? null : (value['offset'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'limit': limit,
      'offset': offset,
    };
  }
}

class FindMany {
  const FindMany({this.where, this.orderBy});

  final String? where;
  final String? orderBy;

  factory FindMany.fromWire(CratestackValueMap value) {
    return FindMany(
      where: value['where'] as String?,
      orderBy: value['orderBy'] as String?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'where': where,
      'orderBy': orderBy,
    };
  }
}

