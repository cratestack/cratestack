import 'dart:convert';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';

Object cratestackRequireWireValue(String ownerName, String fieldName, Object? value) {
  if (value == null) {
    throw FormatException('Missing required field $ownerName.$fieldName');
  }
  return value;
}

typedef CratestackValueMap = Map<String, Object?>;

/// Recursively rewrites decoded containers into the `Map<String, Object?>`
/// / `List<Object?>` shapes the generated `fromWire` decoders expect.
///
/// The `cbor` package hands maps back as `Map<Object?, Object?>` (and
/// nested maps/lists likewise), which is *not* a `Map<String, dynamic>`
/// — casting it directly throws `_Map<Object?, Object?> is not a subtype
/// of Map<String, dynamic>` at runtime. JSON-decoded bodies already have
/// `String` keys, so for them this is a cheap structural walk.
Object? cratestackNormalizeWire(Object? value) {
  // `Uint8List` implements `List<int>`, so it must be matched before the
  // generic `List` branch — otherwise CBOR byte strings (`Bytes` fields)
  // get exploded into per-byte `List<Object?>`, breaking `is Uint8List`
  // decoders and recursing over every byte.
  if (value is Uint8List) {
    return value;
  }
  if (value is Map) {
    return value.map(
      (key, entry) => MapEntry(key.toString(), cratestackNormalizeWire(entry)),
    );
  }
  if (value is List) {
    return value.map(cratestackNormalizeWire).toList(growable: false);
  }
  return value;
}

CratestackValueMap cratestackAsValueMap(Object? value) {
  if (value is CratestackValueMap) {
    return value;
  }
  final map = value as Map;
  return map.map((key, entry) => MapEntry(key.toString(), entry as Object?));
}

List<Object?> cratestackAsValueList(Object? value) {
  if (value is List<Object?>) {
    return value;
  }
  return List<Object?>.from(value as List);
}

String? cratestackCanonicalizeQuery(Map<String, Object?>? queryParameters) {
  if (queryParameters == null || queryParameters.isEmpty) {
    return null;
  }
  return queryParameters.entries
      .where((entry) => entry.value != null)
      .map((entry) {
        final key = Uri.encodeQueryComponent(entry.key);
        final value = Uri.encodeQueryComponent(entry.value.toString());
        return '$key=$value';
      })
      .join('&');
}

class CratestackCallOptions {
  const CratestackCallOptions({this.headers = const <String, String>{}});

  final Map<String, String> headers;
}

class CratestackRequest {
  const CratestackRequest({
    required this.method,
    required this.path,
    this.queryParameters,
    this.body,
    this.headers = const <String, String>{},
  });

  final String method;
  final String path;
  final Map<String, Object?>? queryParameters;
  final Object? body;
  final Map<String, String> headers;
}

const cratestackUseRustTransportExtraKey = 'cratestackUseRustTransport';

abstract interface class CratestackClientAdapter {
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  });
}

class CratestackDioAdapter implements CratestackClientAdapter {
  const CratestackDioAdapter({
    required Dio dio,
    this.useRustTransport = false,
  }) : _dio = dio;

  final Dio _dio;
  final bool useRustTransport;

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    final response = await _dio.request<Object?>(
      request.path,
      data: request.body,
      queryParameters: request.queryParameters,
      options: Options(
        method: request.method,
        headers: {
          ...request.headers,
          ...?options?.headers,
        },
        extra: {
          if (useRustTransport) cratestackUseRustTransportExtraKey: true,
        },
      ),
    );

    return response.data;
  }
}

class CratestackCborDioAdapter implements CratestackClientAdapter {
  const CratestackCborDioAdapter({required Dio dio}) : _dio = dio;

  final Dio _dio;

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    final response = await _dio.request<List<int>>(
      request.path,
      data: _encodeBody(request.body),
      queryParameters: request.queryParameters,
      options: Options(
        method: request.method,
        responseType: ResponseType.bytes,
        headers: {
          'Accept': 'application/cbor, application/json;q=0.9',
          if (request.body != null) 'Content-Type': 'application/cbor',
          ...request.headers,
          ...?options?.headers,
        },
      ),
    );

    final bytes = response.data;
    if (bytes == null || bytes.isEmpty) {
      return null;
    }

    final contentType = response.headers.value(Headers.contentTypeHeader) ?? '';
    if (_isCborContentType(contentType)) {
      return cratestackNormalizeWire(cbor.cbor.decode(Uint8List.fromList(bytes)));
    }

    if (_isJsonContentType(contentType)) {
      return jsonDecode(utf8.decode(bytes));
    }

    throw FormatException(
      'Unsupported response Content-Type $contentType for CratestackCborDioAdapter',
    );
  }
}

Object? _encodeBody(Object? body) {
  if (body == null) {
    return null;
  }
  if (body is Uint8List) {
    return body;
  }
  if (body is List<int>) {
    return Uint8List.fromList(body);
  }
  return Uint8List.fromList(cbor.cbor.encode(body));
}

bool _isCborContentType(String contentType) {
  return contentType.split(';').first.trim() == 'application/cbor';
}

bool _isJsonContentType(String contentType) {
  return contentType.split(';').first.trim() == Headers.jsonContentType;
}
