// Generated CrateStack Dart RPC runtime for `transport rpc` schemas.
//
// Speaks the `/rpc/{op_id}` and `/rpc/batch` URL space defined by
// `cratestack-axum::rpc`. Two adapter impls ship by default:
//
//   * `CratestackRpcDioAdapter`        — JSON over HTTP (the easy default)
//   * `CratestackRpcCborDioAdapter`    — CBOR-by-default with JSON
//                                        negotiation, mirrors the REST
//                                        runtime's adapter pair

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dio/dio.dart';

typedef CratestackValueMap = Map<String, Object?>;

CratestackValueMap cratestackAsValueMap(Object? value) {
  final map = value as Map<String, dynamic>;
  return map.map((key, entry) => MapEntry(key, entry as Object?));
}

List<Object?> cratestackAsValueList(Object? value) {
  return List<Object?>.from(value as List<dynamic>);
}

Object cratestackRequireWireValue(String ownerName, String fieldName, Object? value) {
  if (value == null) {
    throw FormatException('Missing required field $ownerName.$fieldName');
  }
  return value;
}

/// Stable gRPC-style error codes the server emits. Open string union
/// at runtime — a future server-side code lands as a plain string
/// rather than crashing the client.
class CratestackRpcErrorCodes {
  static const String invalidArgument = 'invalid_argument';
  static const String unauthenticated = 'unauthenticated';
  static const String permissionDenied = 'permission_denied';
  static const String notFound = 'not_found';
  static const String conflict = 'conflict';
  static const String failedPrecondition = 'failed_precondition';
  static const String internal = 'internal';
}

/// Wire shape of an RPC error body. Mirrors
/// `cratestack_core::rpc::RpcErrorBody`.
class RpcErrorBody {
  const RpcErrorBody({
    required this.code,
    required this.message,
    this.details,
  });

  final String code;
  final String message;
  final Object? details;

  factory RpcErrorBody.fromWire(Object? value) {
    if (value is! Map) {
      return RpcErrorBody(
        code: CratestackRpcErrorCodes.internal,
        message: value?.toString() ?? 'unknown RPC error',
      );
    }
    final map = value;
    return RpcErrorBody(
      code: (map['code'] as String?) ?? CratestackRpcErrorCodes.internal,
      message: (map['message'] as String?) ?? '',
      details: map['details'],
    );
  }
}

/// Wire shape of a single batch request frame. Mirrors
/// `cratestack_core::rpc::RpcRequest`.
class RpcRequest {
  const RpcRequest({
    required this.id,
    required this.op,
    required this.input,
    this.idem,
  });

  final int id;
  final String op;
  final Object? input;
  final String? idem;

  Map<String, Object?> toWire() => {
        'id': id,
        'op': op,
        'input': input,
        if (idem != null) 'idem': idem,
      };
}

/// Wire shape of a single batch response frame. Mirrors
/// `cratestack_core::rpc::RpcResponseFrame`.
class RpcResponseFrame {
  const RpcResponseFrame({
    required this.id,
    this.output,
    this.error,
  });

  final int id;
  final Object? output;
  final RpcErrorBody? error;

  factory RpcResponseFrame.fromWire(Object? value) {
    final map = cratestackAsValueMap(value);
    final error = map['error'];
    return RpcResponseFrame(
      id: (map['id'] as num).toInt(),
      output: map['output'],
      error: error == null ? null : RpcErrorBody.fromWire(error),
    );
  }
}

/// Thrown by an RPC adapter when a remote call fails with an
/// `RpcErrorBody` payload. Carries the gRPC-style `code` directly so
/// callers can switch on it without parsing.
class CratestackRpcException implements Exception {
  CratestackRpcException({
    required this.status,
    required this.body,
  });

  final int status;
  final RpcErrorBody body;

  String get code => body.code;
  String get message => body.message;
  Object? get details => body.details;

  @override
  String toString() =>
      'CratestackRpcException(status=$status, code=${body.code}, message=${body.message})';
}

class CratestackRpcCallOptions {
  const CratestackRpcCallOptions({
    this.headers = const <String, String>{},
    this.idempotencyKey,
  });

  final Map<String, String> headers;
  final String? idempotencyKey;
}

/// Adapter interface — implementations carry the actual transport
/// (HTTP via Dio, FFI to the Rust runtime, in-process stub, etc.) and
/// the codec choice. The generated API classes only know about this
/// interface; swapping JSON for CBOR is a constructor flip.
abstract interface class CratestackRpcAdapter {
  /// POST /rpc/{op_id} — unary call.
  Future<Object?> call(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  });

  /// POST /rpc/batch — batched calls.
  Future<List<RpcResponseFrame>> batch(
    List<RpcRequest> requests, {
    CratestackRpcCallOptions? options,
  });

  /// POST /rpc/{op_id} — sequence-returning call.
  Stream<Object?> stream(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  });
}

/// JSON-over-HTTP adapter. The easy default.
class CratestackRpcDioAdapter implements CratestackRpcAdapter {
  const CratestackRpcDioAdapter({required Dio dio}) : _dio = dio;

  final Dio _dio;

  @override
  Future<Object?> call(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async {
    try {
      final response = await _dio.post<Object?>(
        '/rpc/${Uri.encodeComponent(opId)}',
        data: input,
        options: Options(
          contentType: Headers.jsonContentType,
          responseType: ResponseType.json,
          headers: {
            ...?options?.headers,
            if (options?.idempotencyKey != null)
              'Idempotency-Key': options!.idempotencyKey!,
          },
        ),
      );
      return response.data;
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }

  @override
  Future<List<RpcResponseFrame>> batch(
    List<RpcRequest> requests, {
    CratestackRpcCallOptions? options,
  }) async {
    try {
      final response = await _dio.post<Object?>(
        '/rpc/batch',
        data: requests.map((r) => r.toWire()).toList(growable: false),
        options: Options(
          contentType: Headers.jsonContentType,
          responseType: ResponseType.json,
          headers: {...?options?.headers},
        ),
      );
      final frames = response.data as List<dynamic>;
      return frames
          .map((frame) => RpcResponseFrame.fromWire(frame))
          .toList(growable: false);
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }

  @override
  Stream<Object?> stream(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async* {
    // JSON adapter — server emits a single JSON array for the full
    // sequence. The CBOR adapter below handles real frame-by-frame
    // streaming over `application/cbor-seq`.
    try {
      final response = await _dio.post<Object?>(
        '/rpc/${Uri.encodeComponent(opId)}',
        data: input,
        options: Options(
          contentType: Headers.jsonContentType,
          responseType: ResponseType.json,
          headers: {
            'Accept': '${Headers.jsonContentType}, application/cbor-seq;q=0.9',
            ...?options?.headers,
          },
        ),
      );
      final items = response.data as List<dynamic>;
      for (final item in items) {
        yield item;
      }
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }
}

/// CBOR-by-default adapter with JSON negotiation. Mirrors the REST
/// runtime's `CratestackCborDioAdapter`.
class CratestackRpcCborDioAdapter implements CratestackRpcAdapter {
  const CratestackRpcCborDioAdapter({required Dio dio}) : _dio = dio;

  final Dio _dio;

  @override
  Future<Object?> call(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async {
    try {
      final response = await _dio.post<List<int>>(
        '/rpc/${Uri.encodeComponent(opId)}',
        data: _encodeBody(input),
        options: Options(
          contentType: 'application/cbor',
          responseType: ResponseType.bytes,
          headers: {
            'Accept': 'application/cbor, application/json;q=0.9',
            if (options?.idempotencyKey != null)
              'Idempotency-Key': options!.idempotencyKey!,
            ...?options?.headers,
          },
        ),
      );
      return _decodeBody(response);
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }

  @override
  Future<List<RpcResponseFrame>> batch(
    List<RpcRequest> requests, {
    CratestackRpcCallOptions? options,
  }) async {
    try {
      final wire =
          requests.map((r) => r.toWire()).toList(growable: false);
      final response = await _dio.post<List<int>>(
        '/rpc/batch',
        data: _encodeBody(wire),
        options: Options(
          contentType: 'application/cbor',
          responseType: ResponseType.bytes,
          headers: {
            'Accept': 'application/cbor, application/json;q=0.9',
            ...?options?.headers,
          },
        ),
      );
      final decoded = _decodeBody(response);
      final frames = decoded as List<dynamic>;
      return frames
          .map((frame) => RpcResponseFrame.fromWire(frame))
          .toList(growable: false);
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }

  @override
  Stream<Object?> stream(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async* {
    // TODO: stream `application/cbor-seq` frame-by-frame. Today we
    //       buffer the body and decode it as a single CBOR array,
    //       which matches the REST runtime's `post_list` behaviour.
    try {
      final response = await _dio.post<List<int>>(
        '/rpc/${Uri.encodeComponent(opId)}',
        data: _encodeBody(input),
        options: Options(
          contentType: 'application/cbor',
          responseType: ResponseType.bytes,
          headers: {
            'Accept': 'application/cbor-seq, application/cbor, application/json;q=0.9',
            ...?options?.headers,
          },
        ),
      );
      final decoded = _decodeBody(response);
      final items = decoded as List<dynamic>;
      for (final item in items) {
        yield item;
      }
    } on DioException catch (error) {
      throw _exceptionFromDio(error);
    }
  }

  Object? _decodeBody(Response<List<int>> response) {
    final bytes = response.data;
    if (bytes == null || bytes.isEmpty) {
      return null;
    }
    final contentType = response.headers.value(Headers.contentTypeHeader) ?? '';
    if (_isCborContentType(contentType)) {
      return cbor.cbor.decode(Uint8List.fromList(bytes));
    }
    if (_isJsonContentType(contentType)) {
      return jsonDecode(utf8.decode(bytes));
    }
    throw FormatException(
      'Unsupported response Content-Type $contentType for CratestackRpcCborDioAdapter',
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

CratestackRpcException _exceptionFromDio(DioException error) {
  final response = error.response;
  final status = response?.statusCode ?? 0;
  Object? data = response?.data;

  // Surface the body if Dio handed it back as bytes (CBOR adapter
  // path); otherwise let RpcErrorBody.fromWire handle the JSON shape.
  if (data is List<int>) {
    final contentType =
        response?.headers.value(Headers.contentTypeHeader) ?? '';
    if (_isCborContentType(contentType)) {
      try {
        data = cbor.cbor.decode(Uint8List.fromList(data));
      } catch (_) {
        data = null;
      }
    } else if (_isJsonContentType(contentType)) {
      try {
        data = jsonDecode(utf8.decode(data));
      } catch (_) {
        data = null;
      }
    }
  }

  return CratestackRpcException(
    status: status,
    body: RpcErrorBody.fromWire(data ?? {'code': 'internal', 'message': error.message ?? ''}),
  );
}

// -----------------------------------------------------------------------------
// `application/cbor-seq` streaming primitives
//
// HTTP-client-agnostic primitives for consuming a cbor-seq response as
// a true Dart `Stream<T>` — items arrive as bytes parse off the wire,
// no full-body buffering. Two pieces:
//
//   * `CborSeqDecoderHandle` — contract a boundary scanner must satisfy.
//     Plug in `FlutterCborSeqDecoder` from `cratestack-client-flutter`
//     (frb-generated, FFI-backed by Rust's `CborSeqChunkDecoder`) for
//     native Flutter apps, or any other impl for non-Flutter contexts.
//
//   * `CborSeqStreamTransformer` — a plain `dart:async`
//     `StreamTransformer<Uint8List, Uint8List>` that wraps a decoder
//     handle. Composes with dio's `ResponseBody.stream`, with
//     `package:http`'s `StreamedResponse.stream`, with `dart:io`
//     `HttpClient`, with a test mock — anything that produces
//     `Stream<Uint8List>`. No opinion on the HTTP client.
//
// Recipe (dio + interceptors + transformer):
//
//   final dio = Dio(BaseOptions(baseUrl: 'https://...'))
//     // Auth header on every request.
//     ..interceptors.add(InterceptorsWrapper(onRequest: (opts, h) {
//       opts.headers['Authorization'] = 'Bearer ${currentToken()}';
//       h.next(opts);
//     }))
//     // Per-request idempotency key for safe retries.
//     ..interceptors.add(InterceptorsWrapper(onRequest: (opts, h) {
//       opts.headers.putIfAbsent('Idempotency-Key', () => Uuid().v4());
//       h.next(opts);
//     }))
//     // Retry transient failures (use `dio_smart_retry` in production).
//     ..interceptors.add(RetryOnTransientInterceptor(maxAttempts: 3));
//
//   final decoder = myFlutterCborSeqDecoder();   // or any impl
//   final response = await dio.post<ResponseBody>(
//     '/rpc/$opId',
//     data: input,
//     options: Options(
//       responseType: ResponseType.stream,
//       contentType: 'application/cbor',
//       headers: {'Accept': 'application/cbor-seq'},
//     ),
//   );
//
//   final items = response.data!.stream
//     .transform(CborSeqStreamTransformer(decoder))
//     .map((bytes) => MyType.fromWire(cbor.cbor.decode(bytes)));
//
//   await for (final item in items) renderRow(item);
//
// The transformer throws `FormatException` if the upstream stream
// closes with a non-empty decoder buffer (truncated final frame),
// which surfaces naturally through `Stream.listen(..., onError: ...)`
// or an `await for` try/catch — no separate error channel.
// -----------------------------------------------------------------------------

/// Contract for a stateful `application/cbor-seq` boundary scanner.
///
/// One method, one accessor, no I/O. Implementations buffer partial
/// frames across chunks and emit the bytes of each complete top-level
/// CBOR item as it becomes available.
///
/// Ship-with-Flutter impl: `FlutterCborSeqDecoder` from
/// `cratestack-client-flutter` (FFI-backed). Pure-Dart impls can be
/// added by hosts that need them (web, server-side Dart, tests).
abstract interface class CborSeqDecoderHandle {
  /// Feed one chunk of bytes from the HTTP response body. Returns the
  /// bytes of every complete top-level CBOR item now available. Any
  /// trailing bytes that don't yet form a complete item stay buffered
  /// for the next call.
  ///
  /// Returns a `Future` because FFI-backed impls hop across an isolate
  /// boundary; in-process impls can return a `SynchronousFuture`.
  Future<List<Uint8List>> feed(Uint8List chunk);

  /// Bytes currently buffered, waiting for frame completion. After the
  /// upstream stream closes, a non-zero value here indicates the
  /// server hung up mid-item — consumers should surface that as a
  /// terminal error.
  int pendingLen();
}

/// `StreamTransformer<Uint8List, Uint8List>` that turns a chunked byte
/// stream into one element per complete cbor-seq item.
///
/// Wraps any [CborSeqDecoderHandle]. Compose with any HTTP client's
/// response stream — dio, `package:http`, `dart:io`, a test mock.
/// Per-item CBOR decoding is the consumer's responsibility (typically
/// `cbor.cbor.decode(bytes)`), so the transformer stays codec-agnostic
/// at the boundary level and keeps decode cost out of the
/// transformer's hot loop.
///
/// Errors flow through the standard Dart stream error channel:
///
///   * decoder errors (malformed CBOR) propagate as the original
///     exception type the decoder throws,
///   * stream-closed-mid-frame raises a [FormatException].
///
/// Cancellation works as expected — `subscription.cancel()` propagates
/// upstream through dio's stream and tears down the HTTP request via
/// the underlying client's cancellation contract.
class CborSeqStreamTransformer
    extends StreamTransformerBase<Uint8List, Uint8List> {
  const CborSeqStreamTransformer(this.decoder);

  final CborSeqDecoderHandle decoder;

  @override
  Stream<Uint8List> bind(Stream<Uint8List> stream) async* {
    await for (final chunk in stream) {
      final items = await decoder.feed(chunk);
      for (final item in items) {
        yield item;
      }
    }
    final pending = decoder.pendingLen();
    if (pending > 0) {
      throw FormatException(
        'cbor-seq stream ended with $pending bytes buffered '
        '(truncated final frame)',
      );
    }
  }
}
