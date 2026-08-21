import 'dart:typed_data';

import 'package:decimal/decimal.dart';

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

enum SortDirection {
  asc('asc'),
  desc('desc');

  const SortDirection(this.wireName);

  final String wireName;

  static SortDirection fromWire(Object? value) {
    final wireName = value as String;
    switch (wireName) {
      case 'asc':
        return SortDirection.asc;
      case 'desc':
        return SortDirection.desc;
    }
    throw ArgumentError.value(wireName, 'value', 'Unknown SortDirection value');
  }

  Object toWire() => wireName;
}

// Shared building blocks for every `<Model>Where`/`<Model>FindMany` pair
// (search-with-filters for procedures — mirrors
// cratestack-core::find_many::FieldFilterInput exactly). Defined once
// here (like Page/PageInfo/PageInput above) rather than per-model; the
// per-model `<Model>Where`/`<Model>SortField`/`<Model>OrderByClause`/
// `<Model>FindMany` classes referencing these are generated below, in
// the data_classes/enum_types loops. Usable only as a procedure
// argument type.
class StringFilter {
  const StringFilter({
    this.eq,
    this.ne,
    this.in$,
    this.lt,
    this.lte,
    this.gt,
    this.gte,
    this.contains,
    this.startsWith,
    this.isNull,
  });

  final String? eq;
  final String? ne;
  final List<String>? in$;
  final String? lt;
  final String? lte;
  final String? gt;
  final String? gte;
  final String? contains;
  final String? startsWith;
  final bool? isNull;

  factory StringFilter.fromWire(CratestackValueMap value) {
    return StringFilter(
      eq: value['eq'] as String?,
      ne: value['ne'] as String?,
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => item as String).toList(growable: false),
      lt: value['lt'] as String?,
      lte: value['lte'] as String?,
      gt: value['gt'] as String?,
      gte: value['gte'] as String?,
      contains: value['contains'] as String?,
      startsWith: value['startsWith'] as String?,
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq,
      'ne': ne,
      'in': in$,
      'lt': lt,
      'lte': lte,
      'gt': gt,
      'gte': gte,
      'contains': contains,
      'startsWith': startsWith,
      'isNull': isNull,
    };
  }
}

class NumberFilter {
  const NumberFilter({this.eq, this.ne, this.in$, this.lt, this.lte, this.gt, this.gte, this.isNull});

  final num? eq;
  final num? ne;
  final List<num>? in$;
  final num? lt;
  final num? lte;
  final num? gt;
  final num? gte;
  final bool? isNull;

  factory NumberFilter.fromWire(CratestackValueMap value) {
    return NumberFilter(
      eq: value['eq'] as num?,
      ne: value['ne'] as num?,
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => item as num).toList(growable: false),
      lt: value['lt'] as num?,
      lte: value['lte'] as num?,
      gt: value['gt'] as num?,
      gte: value['gte'] as num?,
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq,
      'ne': ne,
      'in': in$,
      'lt': lt,
      'lte': lte,
      'gt': gt,
      'gte': gte,
      'isNull': isNull,
    };
  }
}

class BooleanFilter {
  const BooleanFilter({this.eq, this.ne, this.in$, this.isNull});

  final bool? eq;
  final bool? ne;
  final List<bool>? in$;
  final bool? isNull;

  factory BooleanFilter.fromWire(CratestackValueMap value) {
    return BooleanFilter(
      eq: value['eq'] as bool?,
      ne: value['ne'] as bool?,
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => item as bool).toList(growable: false),
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq,
      'ne': ne,
      'in': in$,
      'isNull': isNull,
    };
  }
}

class UuidFilter {
  const UuidFilter({this.eq, this.ne, this.in$, this.lt, this.lte, this.gt, this.gte, this.isNull});

  final String? eq;
  final String? ne;
  final List<String>? in$;
  final String? lt;
  final String? lte;
  final String? gt;
  final String? gte;
  final bool? isNull;

  factory UuidFilter.fromWire(CratestackValueMap value) {
    return UuidFilter(
      eq: value['eq'] as String?,
      ne: value['ne'] as String?,
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => item as String).toList(growable: false),
      lt: value['lt'] as String?,
      lte: value['lte'] as String?,
      gt: value['gt'] as String?,
      gte: value['gte'] as String?,
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq,
      'ne': ne,
      'in': in$,
      'lt': lt,
      'lte': lte,
      'gt': gt,
      'gte': gte,
      'isNull': isNull,
    };
  }
}

class DateTimeFilter {
  const DateTimeFilter({this.eq, this.ne, this.in$, this.lt, this.lte, this.gt, this.gte, this.isNull});

  final DateTime? eq;
  final DateTime? ne;
  final List<DateTime>? in$;
  final DateTime? lt;
  final DateTime? lte;
  final DateTime? gt;
  final DateTime? gte;
  final bool? isNull;

  factory DateTimeFilter.fromWire(CratestackValueMap value) {
    return DateTimeFilter(
      eq: value['eq'] == null ? null : DateTime.parse(value['eq'] as String),
      ne: value['ne'] == null ? null : DateTime.parse(value['ne'] as String),
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => DateTime.parse(item as String)).toList(growable: false),
      lt: value['lt'] == null ? null : DateTime.parse(value['lt'] as String),
      lte: value['lte'] == null ? null : DateTime.parse(value['lte'] as String),
      gt: value['gt'] == null ? null : DateTime.parse(value['gt'] as String),
      gte: value['gte'] == null ? null : DateTime.parse(value['gte'] as String),
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq?.toUtc().toIso8601String(),
      'ne': ne?.toUtc().toIso8601String(),
      'in': in$?.map((item) => item.toUtc().toIso8601String()).toList(growable: false),
      'lt': lt?.toUtc().toIso8601String(),
      'lte': lte?.toUtc().toIso8601String(),
      'gt': gt?.toUtc().toIso8601String(),
      'gte': gte?.toUtc().toIso8601String(),
      'isNull': isNull,
    };
  }
}

// cratestack#498: `Decimal` (`package:decimal`), not `String` — every
// comparison operand is a real arbitrary-precision value, parsed the same
// way regardless of whether the server that produced it is on the
// `decimal-rust-decimal` or `decimal-bigdecimal` backend (see
// `dart_type`'s "Decimal" arm in `crate::dart_types` for why those two
// backends' wire strings otherwise diverge).
class DecimalFilter {
  const DecimalFilter({this.eq, this.ne, this.in$, this.lt, this.lte, this.gt, this.gte, this.isNull});

  final Decimal? eq;
  final Decimal? ne;
  final List<Decimal>? in$;
  final Decimal? lt;
  final Decimal? lte;
  final Decimal? gt;
  final Decimal? gte;
  final bool? isNull;

  factory DecimalFilter.fromWire(CratestackValueMap value) {
    return DecimalFilter(
      eq: value['eq'] == null ? null : Decimal.parse(value['eq'] as String),
      ne: value['ne'] == null ? null : Decimal.parse(value['ne'] as String),
      in$: value['in'] == null
          ? null
          : cratestackAsValueList(value['in']).map((item) => Decimal.parse(item as String)).toList(growable: false),
      lt: value['lt'] == null ? null : Decimal.parse(value['lt'] as String),
      lte: value['lte'] == null ? null : Decimal.parse(value['lte'] as String),
      gt: value['gt'] == null ? null : Decimal.parse(value['gt'] as String),
      gte: value['gte'] == null ? null : Decimal.parse(value['gte'] as String),
      isNull: value['isNull'] as bool?,
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'eq': eq?.toString(),
      'ne': ne?.toString(),
      'in': in$?.map((item) => item.toString()).toList(growable: false),
      'lt': lt?.toString(),
      'lte': lte?.toString(),
      'gt': gt?.toString(),
      'gte': gte?.toString(),
      'isNull': isNull,
    };
  }
}

enum WidgetSortField {
  id('id'),  name('name'),  weight('weight');
  const WidgetSortField(this.wireName);

  final String wireName;

  static WidgetSortField fromWire(Object? value) {
    final wireName = value as String;
    switch (wireName) {
      case 'id':
        return WidgetSortField.id;
      case 'name':
        return WidgetSortField.name;
      case 'weight':
        return WidgetSortField.weight;
    }
    throw ArgumentError.value(wireName, 'value', 'Unknown WidgetSortField value');
  }

  Object toWire() => wireName;
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

class WidgetBuilder {
  int? _id;
  String? _name;
  int? _weight;

  WidgetBuilder id(int? value) {
    _id = value;
    return this;
  }

  WidgetBuilder name(String? value) {
    _name = value;
    return this;
  }

  WidgetBuilder weight(int? value) {
    _weight = value;
    return this;
  }

  Widget build() {
    return Widget(
      id: _id,
      name: _name,
      weight: _weight,
    );
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

class CreateWidgetInputBuilder {
  int? _id;
  bool _idSet = false;
  String? _name;
  bool _nameSet = false;
  int? _weight;

  CreateWidgetInputBuilder id(int value) {
    _id = value;
    _idSet = true;
    return this;
  }

  CreateWidgetInputBuilder name(String value) {
    _name = value;
    _nameSet = true;
    return this;
  }

  CreateWidgetInputBuilder weight(int? value) {
    _weight = value;
    return this;
  }

  CreateWidgetInput build() {
    return CreateWidgetInput(
      id: _idSet ? (_id as int) : (throw StateError('CreateWidgetInput.id is required but was not set')),
      name: _nameSet ? (_name as String) : (throw StateError('CreateWidgetInput.name is required but was not set')),
      weight: _weight,
    );
  }
}

class UpdateWidgetInput {
  const UpdateWidgetInput({
this.name,
this.weight,
    this.weightIsSet = false,
  });

  final String? name;
  final int? weight;
  // Outer "did the caller touch this field" flag, alongside
  // `weight`'s own value (the inner "new value, or `null`
  // to clear") — the Dart analogue of the generated Rust client's
  // `Option<Option<T>>` for this nullable-column field (cratestack#663).
  // `false`/omitted means untouched (`weight` stays off the
  // wire); `true` with `weight == null` means an explicit
  // clear (serializes as `null`); `true` with a non-null value means set.
  final bool weightIsSet;

  factory UpdateWidgetInput.fromWire(CratestackValueMap value) {
    return UpdateWidgetInput(
      name: value['name'] == null ? null : value['name'] as String,
      weight: value['weight'] == null ? null : (value['weight'] as num).toInt(),
      weightIsSet: value.containsKey('weight'),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      if (name != null) 'name': name,
      if (weightIsSet) 'weight': weight,
    };
  }
}

class UpdateWidgetInputBuilder {
  String? _name;
  int? _weight;
  bool _weightSet = false;

  UpdateWidgetInputBuilder name(String? value) {
    _name = value;
    return this;
  }

  UpdateWidgetInputBuilder weight(int? value) {
    _weight = value;
    _weightSet = true;
    return this;
  }

  UpdateWidgetInput build() {
    return UpdateWidgetInput(
      name: _name,
      weight: _weight,
      weightIsSet: _weightSet,
    );
  }
}

class WidgetWhere {
  const WidgetWhere({
this.id,
this.name,
this.weight,
  });

  final NumberFilter? id;
  final StringFilter? name;
  final NumberFilter? weight;

  factory WidgetWhere.fromWire(CratestackValueMap value) {
    return WidgetWhere(
      id: value['id'] == null ? null : NumberFilter.fromWire(cratestackAsValueMap(value['id'])),
      name: value['name'] == null ? null : StringFilter.fromWire(cratestackAsValueMap(value['name'])),
      weight: value['weight'] == null ? null : NumberFilter.fromWire(cratestackAsValueMap(value['weight'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'id': id?.toWire(),
      'name': name?.toWire(),
      'weight': weight?.toWire(),
    };
  }
}

class WidgetWhereBuilder {
  NumberFilter? _id;
  StringFilter? _name;
  NumberFilter? _weight;

  WidgetWhereBuilder id(NumberFilter? value) {
    _id = value;
    return this;
  }

  WidgetWhereBuilder name(StringFilter? value) {
    _name = value;
    return this;
  }

  WidgetWhereBuilder weight(NumberFilter? value) {
    _weight = value;
    return this;
  }

  WidgetWhere build() {
    return WidgetWhere(
      id: _id,
      name: _name,
      weight: _weight,
    );
  }
}

class WidgetOrderByClause {
  const WidgetOrderByClause({
required this.field,
required this.direction,
  });

  final WidgetSortField field;
  final SortDirection direction;

  factory WidgetOrderByClause.fromWire(CratestackValueMap value) {
    return WidgetOrderByClause(
      field: WidgetSortField.fromWire(cratestackRequireWireValue('WidgetOrderByClause', 'field', value['field'])),
      direction: SortDirection.fromWire(cratestackRequireWireValue('WidgetOrderByClause', 'direction', value['direction'])),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'field': field.toWire(),
      'direction': direction.toWire(),
    };
  }
}

class WidgetOrderByClauseBuilder {
  WidgetSortField? _field;
  bool _fieldSet = false;
  SortDirection? _direction;
  bool _directionSet = false;

  WidgetOrderByClauseBuilder field(WidgetSortField value) {
    _field = value;
    _fieldSet = true;
    return this;
  }

  WidgetOrderByClauseBuilder direction(SortDirection value) {
    _direction = value;
    _directionSet = true;
    return this;
  }

  WidgetOrderByClause build() {
    return WidgetOrderByClause(
      field: _fieldSet ? (_field as WidgetSortField) : (throw StateError('WidgetOrderByClause.field is required but was not set')),
      direction: _directionSet ? (_direction as SortDirection) : (throw StateError('WidgetOrderByClause.direction is required but was not set')),
    );
  }
}

class WidgetFindMany {
  const WidgetFindMany({
this.where,
this.orderBy,
  });

  final WidgetWhere? where;
  final List<WidgetOrderByClause>? orderBy;

  factory WidgetFindMany.fromWire(CratestackValueMap value) {
    return WidgetFindMany(
      where: value['where'] == null ? null : WidgetWhere.fromWire(cratestackAsValueMap(value['where'])),
      orderBy: value['orderBy'] == null ? null : cratestackAsValueList(value['orderBy']).map((item) => WidgetOrderByClause.fromWire(cratestackAsValueMap(item))).toList(growable: false),
    );
  }

  CratestackValueMap toWire() {
    return <String, Object?>{
      'where': where?.toWire(),
      'orderBy': orderBy?.map((item) => item.toWire()).toList(growable: false),
    };
  }
}

class WidgetFindManyBuilder {
  WidgetWhere? _where;
  List<WidgetOrderByClause>? _orderBy;

  WidgetFindManyBuilder where(WidgetWhere? value) {
    _where = value;
    return this;
  }

  WidgetFindManyBuilder orderBy(List<WidgetOrderByClause>? value) {
    _orderBy = value;
    return this;
  }

  WidgetFindMany build() {
    return WidgetFindMany(
      where: _where,
      orderBy: _orderBy,
    );
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

class EchoNameArgsBuilder {
  String? _name;
  bool _nameSet = false;

  EchoNameArgsBuilder name(String value) {
    _name = value;
    _nameSet = true;
    return this;
  }

  EchoNameArgs build() {
    return EchoNameArgs(
      name: _nameSet ? (_name as String) : (throw StateError('EchoNameArgs.name is required but was not set')),
    );
  }
}

class ProjectedWidget {
  const ProjectedWidget.fromWire(this._value);

  final CratestackValueMap _value;

  int? get id => _value['id'] == null ? null : (_value['id'] as num).toInt();

  String? get name => _value['name'] == null ? null : _value['name'] as String;

  int? get weight => _value['weight'] == null ? null : (_value['weight'] as num).toInt();

}

