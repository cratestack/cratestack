import 'dart:typed_data';

import 'runtime.dart';

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

  final List<T> items;
  final int? totalCount;
  final PageInfo pageInfo;

  factory Page.fromWire(
    CratestackValueMap value, {
    required T Function(Object? item) decodeItem,
  }) {
    return Page<T>(
      items: cratestackAsValueList(cratestackRequireWireValue('Page', 'items', value['items']))
          .map((item) => decodeItem(item))
          .toList(growable: false),
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

class Widget {
  const Widget({
this.id,
this.name,
this.weight,
  });

  final int? id;
  final String? name;
  final int? weight;

  factory Widget.fromWire(CratestackValueMap value) {
    return Widget(
      id: value['id'] == null ? null : (value['id'] as num).toInt(),
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'weight': weight,
    };
  }
}

class CreateWidgetInput {
  const CreateWidgetInput({
required this.id,
required this.name,
this.weight,
  });

  final int id;
  final String name;
  final int? weight;

  factory CreateWidgetInput.fromWire(CratestackValueMap value) {
    return CreateWidgetInput(
      id: (cratestackRequireWireValue('CreateWidgetInput', 'id', value['id']) as num).toInt(),
      name: cratestackRequireWireValue('CreateWidgetInput', 'name', value['name']) as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'weight': weight,
    };
  }
}

class UpdateWidgetInput {
  const UpdateWidgetInput({
this.name,
this.weight,
  });

  final String? name;
  final int? weight;

  factory UpdateWidgetInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetInput(
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
      'weight': weight,
    };
  }
}

class EchoNameArgs {
  const EchoNameArgs({
required this.name,
  });

  final String name;

  factory EchoNameArgs.fromWire(CratestackValueMap value) {
    return EchoNameArgs(
      name: cratestackRequireWireValue('EchoNameArgs', 'name', value['name']) as String,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'name': name,
    };
  }
}

class ProjectedWidget {
  const ProjectedWidget.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get name => _value['name'] == null ? null : _value['name'] as String;

  int? get weight => _value['weight'] == null ? null : (_value['weight'] as num).toInt();

}

