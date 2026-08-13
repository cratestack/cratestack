// Generated CrateStack Dart gRPC runtime for `transport grpc` schemas
// (ticket #210, the Dart counterpart to the TypeScript gRPC-Web runtime,
// `docs/design/protobuf.md` §5/§9). Hand-rolled protobuf *message* codec
// for the bounded field-type set §4.1 maps `.cstack` scalars onto (int64,
// double, bool, string, bytes, `Timestamp`, enum, embedded message,
// repeated) — no `protoc`/`protoc-gen-dart` step, no protobuf-runtime
// dependency. Framing/HTTP-2/trailers are handled by `package:grpc`
// itself: unlike the gRPC-Web runtime, this file never touches raw
// sockets or frame headers.

import 'dart:convert';
import 'dart:typed_data';

import 'package:grpc/grpc.dart';

typedef CratestackValueMap = Map<String, Object?>;

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

Object cratestackRequireWireValue(String ownerName, String fieldName, Object? value) {
  if (value == null) {
    throw FormatException('Missing required field $ownerName.$fieldName');
  }
  return value;
}

// ---------------------------------------------------------------------
// Wire primitives: varint, fixed64, length-delimited framing. Plain 64-bit
// `int` throughout (the Dart VM/AOT native target this client is scoped
// to has 64-bit `int`; this mirrors `.cstack Int` already mapping to
// Dart's native `int` elsewhere in this generator).
// ---------------------------------------------------------------------

const int _wireVarint = 0;
const int _wireFixed64 = 1;
const int _wireLengthDelimited = 2;

Uint8List _encodeVarint(int value) {
  final bytes = <int>[];
  var v = value;
  while (true) {
    final byte = v & 0x7f;
    v = v >>> 7;
    if (v == 0) {
      bytes.add(byte);
      break;
    }
    bytes.add(byte | 0x80);
  }
  return Uint8List.fromList(bytes);
}

/// Returns `(value, nextOffset)`.
(int, int) _decodeVarint(Uint8List bytes, int offset) {
  var result = 0;
  var shift = 0;
  var pos = offset;
  while (true) {
    if (pos >= bytes.length) {
      throw CratestackGrpcCodecError('truncated varint in message body');
    }
    final byte = bytes[pos];
    pos += 1;
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) == 0) {
      break;
    }
    shift += 7;
  }
  return (result, pos);
}

Uint8List _encodeTag(int fieldNumber, int wireType) =>
    _encodeVarint((fieldNumber << 3) | wireType);

Uint8List _encodeFixed64Double(double value) {
  final bytes = ByteData(8);
  bytes.setFloat64(0, value, Endian.little);
  return bytes.buffer.asUint8List();
}

double _decodeFixed64Double(Uint8List bytes, int offset) {
  final view = ByteData.sublistView(bytes, offset, offset + 8);
  return view.getFloat64(0, Endian.little);
}

Uint8List _concatBytes(List<List<int>> chunks) {
  final total = chunks.fold(0, (sum, chunk) => sum + chunk.length);
  final out = Uint8List(total);
  var offset = 0;
  for (final chunk in chunks) {
    out.setAll(offset, chunk);
    offset += chunk.length;
  }
  return out;
}

/// Thrown for malformed wire bytes (truncated varint, unsupported wire
/// type, ...). Distinct from [CratestackGrpcError] (a real server-emitted
/// gRPC status) — this means the bytes themselves didn't parse.
class CratestackGrpcCodecError extends Error {
  CratestackGrpcCodecError(this.message);

  final String message;

  @override
  String toString() => 'CratestackGrpcCodecError: $message';
}

// ---------------------------------------------------------------------
// google.protobuf.Timestamp <-> ISO 8601 string (`.cstack DateTime`'s
// Dart wire shape everywhere else in this generator — `wire_encode.rs`'s
// `.toUtc().toIso8601String()`, `wire_decode.rs`'s `DateTime.parse(...)`).
// A fixed 2-field message (`seconds: int64 = 1`, `nanos: int32 = 2`),
// built in directly rather than through the generic message registry,
// since it's the one message shape every schema shares regardless of
// what it declares. Millisecond precision only, matching the TypeScript
// gRPC-Web runtime's own `Date`-based (millisecond) implementation.
// ---------------------------------------------------------------------

Uint8List _encodeTimestamp(String iso) {
  final ms = DateTime.parse(iso).toUtc().millisecondsSinceEpoch;
  final seconds = ms ~/ 1000;
  final nanos = (ms % 1000) * 1000000;
  final parts = <List<int>>[];
  if (seconds != 0) {
    parts.add(_encodeTag(1, _wireVarint));
    parts.add(_encodeVarint(seconds));
  }
  if (nanos != 0) {
    parts.add(_encodeTag(2, _wireVarint));
    parts.add(_encodeVarint(nanos));
  }
  return _concatBytes(parts);
}

String _decodeTimestamp(Uint8List bytes) {
  var seconds = 0;
  var nanos = 0;
  for (final field in _iterateFields(bytes)) {
    if (field.fieldNumber == 1 && field.wireType == _wireVarint) {
      seconds = _decodeVarint(bytes, field.offset).$1;
    } else if (field.fieldNumber == 2 && field.wireType == _wireVarint) {
      nanos = _decodeVarint(bytes, field.offset).$1;
    }
  }
  final ms = seconds * 1000 + nanos ~/ 1000000;
  return DateTime.fromMillisecondsSinceEpoch(ms, isUtc: true).toIso8601String();
}

// ---------------------------------------------------------------------
// Data-driven message codec — every generated field-descriptor table in
// `apis.dart` is walked by these two functions, so there is exactly one
// encoder/decoder implementation regardless of how many message shapes a
// schema declares.
// ---------------------------------------------------------------------

/// A single field on a generated message's descriptor table. Mirrors
/// `cratestack-client-dart::grpc::wire::GrpcFieldDescriptor` field for
/// field — see that Rust type's doc for what each property means.
class CratestackGrpcFieldDescriptor {
  const CratestackGrpcFieldDescriptor({
    required this.property,
    required this.number,
    required this.kind,
    this.repeated = false,
    this.refName,
    this.defaultsWhenAbsent = false,
  });

  final String property;
  final int number;
  final String kind;
  final bool repeated;
  final String? refName;
  final bool defaultsWhenAbsent;
}

typedef CratestackGrpcMessageRegistry = Map<String, List<CratestackGrpcFieldDescriptor>>;
typedef CratestackGrpcEnumRegistry = Map<String, Map<String, int>>;

Object? _zeroValue(String kind) {
  switch (kind) {
    case 'int64':
    case 'double':
      return 0;
    case 'bool':
      return false;
    case 'string':
      return '';
    case 'bytes':
      return Uint8List(0);
    case 'timestamp':
      return DateTime.fromMillisecondsSinceEpoch(0, isUtc: true).toIso8601String();
    default:
      return null;
  }
}

List<CratestackGrpcFieldDescriptor> _messageFields(
  CratestackGrpcMessageRegistry messages,
  String? refName,
) {
  final fields = messages[refName ?? ''];
  if (fields == null) {
    throw CratestackGrpcCodecError("unknown message type '$refName'");
  }
  return fields;
}

(int, List<int>) _encodeScalar(
  String kind,
  Object? value,
  String? refName,
  CratestackGrpcEnumRegistry enums,
  CratestackGrpcMessageRegistry messages,
) {
  switch (kind) {
    case 'int64':
      return (_wireVarint, _encodeVarint(value as int));
    case 'bool':
      return (_wireVarint, _encodeVarint((value as bool) ? 1 : 0));
    case 'double':
      return (_wireFixed64, _encodeFixed64Double((value as num).toDouble()));
    case 'string':
      return (_wireLengthDelimited, utf8.encode(value as String));
    case 'bytes':
      return (_wireLengthDelimited, (value as List).cast<int>());
    case 'timestamp':
      return (_wireLengthDelimited, _encodeTimestamp(value as String));
    case 'enum':
      final table = enums[refName ?? ''] ?? const {};
      final number = table[value as String];
      if (number == null) {
        throw CratestackGrpcCodecError("unknown enum value '$value' for $refName");
      }
      return (_wireVarint, _encodeVarint(number));
    case 'message':
      final fields = _messageFields(messages, refName);
      return (
        _wireLengthDelimited,
        encodeMessage(cratestackAsValueMap(value), fields, messages, enums),
      );
    default:
      throw CratestackGrpcCodecError("unknown wire kind '$kind'");
  }
}

/// Encodes a `Map<String, Object?>` against a generated field-descriptor
/// table. `null`/absent on a scalar field omits it (proto3 explicit
/// presence, `docs/design/protobuf.md` §4.4); an empty list on a
/// repeated field also omits it (proto3 repeated fields have no separate
/// presence bit — an empty and an absent list are the same bytes either
/// way).
Uint8List encodeMessage(
  CratestackValueMap value,
  List<CratestackGrpcFieldDescriptor> descriptors,
  CratestackGrpcMessageRegistry messages,
  CratestackGrpcEnumRegistry enums,
) {
  final parts = <List<int>>[];
  for (final field in descriptors) {
    final raw = value[field.property];
    if (raw == null) {
      continue;
    }
    final values = field.repeated ? cratestackAsValueList(raw) : [raw];
    for (final item in values) {
      final (wireType, bytes) =
          _encodeScalar(field.kind, item, field.refName, enums, messages);
      parts.add(_encodeTag(field.number, wireType));
      if (wireType == _wireLengthDelimited) {
        parts.add(_encodeVarint(bytes.length));
      }
      parts.add(bytes);
    }
  }
  return _concatBytes(parts);
}

class _RawField {
  const _RawField(this.fieldNumber, this.wireType, this.offset, this.next);

  final int fieldNumber;
  final int wireType;
  final int offset;
  final int next;
}

/// Walks a message's top-level fields (not recursive — nested messages
/// are decoded by a fresh call once their length-delimited span is
/// sliced out), yielding each field's wire type and the byte offset its
/// value starts at.
Iterable<_RawField> _iterateFields(Uint8List bytes) sync* {
  var pos = 0;
  while (pos < bytes.length) {
    final (tag, afterTag) = _decodeVarint(bytes, pos);
    final fieldNumber = tag >>> 3;
    final wireType = tag & 0x7;
    var valueStart = afterTag;
    int next;
    if (wireType == _wireVarint) {
      next = _decodeVarint(bytes, valueStart).$2;
    } else if (wireType == _wireFixed64) {
      next = valueStart + 8;
    } else if (wireType == _wireLengthDelimited) {
      final (len, afterLen) = _decodeVarint(bytes, valueStart);
      valueStart = afterLen;
      next = afterLen + len;
    } else {
      throw CratestackGrpcCodecError('unsupported wire type $wireType in message body');
    }
    yield _RawField(fieldNumber, wireType, valueStart, next);
    pos = next;
  }
}

Object? _decodeScalar(
  String kind,
  Uint8List bytes,
  String? refName,
  CratestackGrpcMessageRegistry messages,
  CratestackGrpcEnumRegistry enums,
) {
  switch (kind) {
    case 'int64':
      return _decodeVarint(bytes, 0).$1;
    case 'bool':
      return _decodeVarint(bytes, 0).$1 != 0;
    case 'double':
      return _decodeFixed64Double(bytes, 0);
    case 'string':
      return utf8.decode(bytes);
    case 'bytes':
      return bytes;
    case 'timestamp':
      return _decodeTimestamp(bytes);
    case 'enum':
      final table = enums[refName ?? ''] ?? const {};
      final number = _decodeVarint(bytes, 0).$1;
      for (final entry in table.entries) {
        if (entry.value == number) {
          return entry.key;
        }
      }
      return number.toString();
    case 'message':
      final fields = _messageFields(messages, refName);
      return decodeMessage(bytes, fields, messages, enums);
    default:
      throw CratestackGrpcCodecError("unknown wire kind '$kind'");
  }
}

/// Decodes protobuf message bytes into a `Map<String, Object?>` keyed by
/// each field's `property` — chosen at generation time
/// (`cratestack-client-dart::grpc::messages`) to line up with the wire
/// keys the already-generated `<Message>.fromWire(map)` factories in
/// `models.dart` expect, so the result of this function can be passed
/// straight to one without any reshaping.
CratestackValueMap decodeMessage(
  Uint8List bytes,
  List<CratestackGrpcFieldDescriptor> descriptors,
  CratestackGrpcMessageRegistry messages,
  CratestackGrpcEnumRegistry enums,
) {
  final byNumber = <int, CratestackGrpcFieldDescriptor>{
    for (final field in descriptors) field.number: field,
  };
  final result = <String, Object?>{};
  for (final field in descriptors) {
    if (field.repeated) {
      result[field.property] = <Object?>[];
    } else if (field.defaultsWhenAbsent) {
      result[field.property] = _zeroValue(field.kind);
    }
  }
  for (final raw in _iterateFields(bytes)) {
    final field = byNumber[raw.fieldNumber];
    if (field == null) {
      continue; // Unknown field (forward-compat schema drift) — skip.
    }
    final valueBytes = bytes.sublist(raw.offset, raw.next);
    final decoded = _decodeScalar(field.kind, valueBytes, field.refName, messages, enums);
    if (field.repeated) {
      (result[field.property] as List<Object?>).add(decoded);
    } else {
      result[field.property] = decoded;
    }
  }
  return result;
}

// ---------------------------------------------------------------------
// Errors — the numeric gRPC `status` on `package:grpc`'s `GrpcError`
// mapped to the same friendly string codes the TypeScript gRPC-Web
// runtime uses (`grpc-web-runtime.ts.j2`'s `GRPC_STATUS_TO_CODE`), via
// the inverse of `cratestack-grpc::error::rpc_code_to_tonic_code` — every
// code that table produces round-trips back to its original string; an
// unrecognized number falls back to its decimal string form rather than
// losing information.
// ---------------------------------------------------------------------

const Map<int, String> _grpcStatusToCode = {
  1: 'canceled',
  2: 'unknown',
  3: 'invalid_argument',
  4: 'deadline_exceeded',
  5: 'not_found',
  6: 'conflict',
  7: 'permission_denied',
  9: 'failed_precondition',
  12: 'unimplemented',
  13: 'internal',
  14: 'unavailable',
  16: 'unauthenticated',
};

String cratestackGrpcStatusToErrorCode(int status) =>
    _grpcStatusToCode[status] ?? status.toString();

/// Thrown by [CratestackGrpcRuntime] when a call fails with a real gRPC
/// status. Wraps `package:grpc`'s `GrpcError`, surfacing the same
/// friendly `code` string the TypeScript gRPC-Web client's
/// `CratestackGrpcError` does, so both generated clients report the same
/// string for the same server-side error.
class CratestackGrpcError implements Exception {
  const CratestackGrpcError(this.status, this.message);

  factory CratestackGrpcError.fromGrpcError(GrpcError error) {
    return CratestackGrpcError(error.code, error.message ?? '');
  }

  final int status;
  final String message;

  String get code => cratestackGrpcStatusToErrorCode(status);

  @override
  String toString() =>
      'CratestackGrpcError(code=$code, status=$status, message=$message)';
}

// ---------------------------------------------------------------------
// The runtime itself — a thin `package:grpc` `Client` subclass. Real
// HTTP/2 framing, headers, and trailers are `package:grpc`'s job; this
// class only builds a fresh `ClientMethod` per call (wiring the codec
// above into `package:grpc`'s `List<int> Function(Q)`/`R Function(List<int>)`
// serializer seam, no `GeneratedMessage` base class required) and
// translates a thrown `GrpcError` into `CratestackGrpcError`.
// ---------------------------------------------------------------------

class CratestackGrpcRuntime extends Client {
  CratestackGrpcRuntime(
    this.channel, {
    CallOptions? options,
    Iterable<ClientInterceptor>? interceptors,
  }) : super(channel, options: options, interceptors: interceptors);

  /// The channel this runtime was constructed with. `Client`'s own copy is
  /// private, so this is kept here instead — [shutdown]/[terminate] need
  /// it, and a caller may want it directly (e.g. to construct another
  /// `Client` subclass sharing the same connection).
  final ClientChannel channel;

  /// Convenience constructor: plaintext HTTP/2 (h2c) by default, matching
  /// a `cratestack-grpc` server's own plaintext-only setup
  /// (`docs/design/protobuf.md` §9). Pass a `credentials` built from
  /// `ChannelCredentials.secure(...)` for a TLS-terminated endpoint, or
  /// construct a [ClientChannel] directly and use the primary constructor
  /// for full control (Unix domain sockets, custom `ChannelOptions`, ...).
  factory CratestackGrpcRuntime.host(
    String host, {
    int port = 443,
    ChannelCredentials credentials = const ChannelCredentials.insecure(),
    CallOptions? options,
  }) {
    return CratestackGrpcRuntime(
      ClientChannel(host, port: port, options: ChannelOptions(credentials: credentials)),
      options: options,
    );
  }

  /// Gracefully closes the underlying channel: RPCs already in flight are
  /// allowed to finish first. Without this, nothing closes the HTTP/2
  /// connection `.host()`/the primary constructor opened — a short-lived
  /// process (a script, a test) would otherwise hang on exit.
  Future<void> shutdown() => channel.shutdown();

  /// Immediately closes the underlying channel, terminating any RPCs
  /// already in flight. Prefer [shutdown] unless an immediate close is
  /// actually required.
  Future<void> terminate() => channel.terminate();

  /// Unary gRPC call: encodes `request` against `requestFields`, invokes
  /// `path` over the underlying channel, and decodes the single response
  /// message against `responseFields`. A non-OK gRPC status raises
  /// [CratestackGrpcError].
  Future<CratestackValueMap> unary(
    String path,
    CratestackValueMap request,
    List<CratestackGrpcFieldDescriptor> requestFields,
    List<CratestackGrpcFieldDescriptor> responseFields,
    CratestackGrpcMessageRegistry messages,
    CratestackGrpcEnumRegistry enums, {
    CallOptions? options,
  }) async {
    final method = ClientMethod<CratestackValueMap, CratestackValueMap>(
      path,
      (value) => encodeMessage(value, requestFields, messages, enums),
      (bytes) => decodeMessage(
        bytes is Uint8List ? bytes : Uint8List.fromList(bytes),
        responseFields,
        messages,
        enums,
      ),
    );
    try {
      return await $createUnaryCall(method, request, options: options);
    } on GrpcError catch (error) {
      throw CratestackGrpcError.fromGrpcError(error);
    }
  }
}
