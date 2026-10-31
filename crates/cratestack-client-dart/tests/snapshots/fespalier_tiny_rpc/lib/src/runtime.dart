// Generated CrateStack Dart RPC runtime for `transport rpc` schemas.
//
// Speaks the `/rpc/{op_id}` and `/rpc/batch` URL space defined by
// `cratestack-axum::rpc`. One adapter ships, written over the HTTP seam
// (`CratestackHttpTransport`, below):
//
//   * `CratestackRpcHttpAdapter`  — CBOR or JSON on the wire
//                                   (`CratestackWireFormat`), any transport
//
// plus `CratestackFespalierTransport` (a separate file,
// `fespalier_transport.dart`) for fespalier's `HttpTransport`.

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;

import 'constants.dart';

// -----------------------------------------------------------------------------
// The HTTP seam (cratestack#1204)
//
// Everything the generated adapters need from an HTTP stack, and nothing
// else: pure Dart, no dependency. An app that owns its HTTP stack (a signing
// layer, a captive-portal check, spans, a different client) implements
// `CratestackHttpTransport` once and hands it to `CratestackHttpAdapter` /
// `CratestackRpcHttpAdapter`; the adapters do the codec work, the headers and
// the error mapping over it. Retries, deadlines and interception stay the
// transport's job.
// -----------------------------------------------------------------------------

/// Sends one HTTP request and returns the answer as soon as the status line
/// and headers are in; the body is still a stream at that point.
///
/// Contract:
///
///  * Resolve with the response for EVERY HTTP status. A 4xx/5xx answer is
///    not an error here: the adapter maps it (`CratestackRestException` over
///    REST, `CratestackRpcException` over RPC).
///  * Fail with a [CratestackTransportException] when there is no answer:
///    [CratestackTransportUnsent] when the request provably never left,
///    [CratestackTransportAborted] when [CratestackHttpRequest.abort]
///    completed, [CratestackTransportTimedOut] for a deadline, and
///    [CratestackTransportBroken] for anything else.
///  * Send [CratestackHttpRequest.body] exactly as given (the bytes a signer
///    layered above this one saw), and send no header the request does not
///    carry beyond what the platform adds by itself.
abstract interface class CratestackHttpTransport {
  Future<CratestackHttpResponse> send(CratestackHttpRequest request);
}

/// One request, fully encoded: the codec work is already done.
final class CratestackHttpRequest {
  const CratestackHttpRequest({
    required this.method,
    required this.uri,
    this.headers = const <String, String>{},
    this.body,
    this.abort,
    this.extra = const <String, Object?>{},
  });

  /// Upper-case HTTP method (`GET`, `POST`, ...).
  final String method;

  /// Absolute URI, query string included.
  final Uri uri;

  /// Request headers, names as the adapter wrote them.
  final Map<String, String> headers;

  /// The exact body bytes, or `null` for a request without a body.
  final Uint8List? body;

  /// Completing this future cancels the send, whether the request is still
  /// connecting, uploading, waiting for the answer or streaming the body.
  /// The transport then fails with [CratestackTransportAborted].
  final Future<void>? abort;

  /// Transport-local attributes that are NOT sent on the wire (for example
  /// [cratestackUseRustTransportExtraKey]). A transport forwards the ones it
  /// understands to its own machinery and ignores the rest.
  final Map<String, Object?> extra;
}

/// The answer to a [CratestackHttpRequest].
final class CratestackHttpResponse {
  /// [headers] names are lower-cased; a repeated header is joined with `, `.
  CratestackHttpResponse({
    required this.status,
    required Map<String, String> headers,
    required this.body,
  }) : headers = Map<String, String>.unmodifiable(<String, String>{
         for (final entry in headers.entries) entry.key.toLowerCase(): entry.value,
       });

  /// A fully buffered answer: handy for tests and for transports that have
  /// already read the body.
  factory CratestackHttpResponse.buffered({
    required int status,
    Map<String, String> headers = const <String, String>{},
    List<int> body = const <int>[],
  }) {
    return CratestackHttpResponse(
      status: status,
      headers: headers,
      body: Stream<Uint8List>.value(Uint8List.fromList(body)),
    );
  }

  final int status;

  /// Response headers, names lower-cased.
  final Map<String, String> headers;

  /// The body, as the transport received it. Single-subscription: read it
  /// once, either through [bytes] or by listening to it directly.
  final Stream<Uint8List> body;

  /// Reads the whole [body]. Consumes it.
  Future<Uint8List> bytes() async {
    final builder = BytesBuilder(copy: false);
    await for (final chunk in body) {
      builder.add(chunk);
    }
    return builder.takeBytes();
  }
}

/// A request that produced no HTTP answer. Never a `DioException` or any other
/// stack-specific type: a caller that catches this works with every transport.
sealed class CratestackTransportException implements Exception {
  const CratestackTransportException({this.cause, this.message});

  /// The stack-specific error that caused this, when there is one.
  final Object? cause;

  final String? message;

  String get _kind;

  @override
  String toString() {
    final detail = message ?? cause;
    return detail == null ? _kind : '$_kind: $detail';
  }
}

/// The caller cancelled the call through [CratestackHttpRequest.abort].
final class CratestackTransportAborted extends CratestackTransportException {
  const CratestackTransportAborted({super.cause, super.message});

  @override
  String get _kind => 'CratestackTransportAborted';
}

/// The request provably never left this device: the connection was refused,
/// the host did not resolve, the connect timed out, or TLS failed before any
/// request byte was written. Always safe to send again, even a non-idempotent
/// call.
final class CratestackTransportUnsent extends CratestackTransportException {
  const CratestackTransportUnsent({super.cause, super.message});

  @override
  String get _kind => 'CratestackTransportUnsent';
}

/// A deadline expired after the request may have left. The server may have
/// acted on it.
final class CratestackTransportTimedOut extends CratestackTransportException {
  const CratestackTransportTimedOut({super.cause, super.message});

  @override
  String get _kind => 'CratestackTransportTimedOut';
}

/// Anything else that went wrong without an answer (a dropped connection
/// mid-request or mid-body, a malformed response). The server may have acted
/// on the request.
final class CratestackTransportBroken extends CratestackTransportException {
  const CratestackTransportBroken({super.cause, super.message});

  @override
  String get _kind => 'CratestackTransportBroken';
}

// -----------------------------------------------------------------------------
// Wire codecs and the plumbing both HTTP adapters share (cratestack#1204)
//
// One copy, included by the REST runtime and the RPC runtime alike, so the
// two adapters cannot drift on the CBOR/JSON choice, on dispatching a
// response by its `Content-Type`, or on how a URI and the headers are built.
// -----------------------------------------------------------------------------

/// The wire format a request body is written in and an `Accept` header
/// prefers. Responses are always decoded by their own `Content-Type`, so an
/// adapter set to one format still reads the other.
enum CratestackWireFormat {
  /// `application/cbor`, the framework's default wire format.
  cbor,

  /// `application/json`.
  json,
}

String _cratestackMimeOf(String? contentType) =>
    (contentType ?? '').split(';').first.trim().toLowerCase();

bool _isCborContentType(String? contentType) =>
    _cratestackMimeOf(contentType) == 'application/cbor';

bool _isJsonContentType(String? contentType) =>
    _cratestackMimeOf(contentType) == 'application/json';

/// Encodes a request body in [format]. `null` stays `null` (no body), and
/// bytes are sent as they are.
Future<Uint8List?> _encodeBody(CratestackWireFormat format, Object? body) async {
  if (body == null) {
    return null;
  }
  if (body is Uint8List) {
    return body;
  }
  if (body is List<int>) {
    return Uint8List.fromList(body);
  }
  switch (format) {
    case CratestackWireFormat.json:
      return Uint8List.fromList(utf8.encode(jsonEncode(body)));
    case CratestackWireFormat.cbor:
      return Uint8List.fromList(cbor.cbor.encode(body));
  }
}

/// Decodes a CBOR body to the JSON-plain value graph the generated `fromWire`
/// decoders expect.
Future<Object?> _decodeCbor(Uint8List bytes) async {
  return cratestackNormalizeWire(cbor.cbor.decode(bytes));
}

/// Decodes a response body by its `Content-Type`: an empty body is `null`,
/// CBOR and JSON are decoded, anything else is a [FormatException].
Future<Object?> _decodeBody(Uint8List bytes, String? contentType, String owner) async {
  if (bytes.isEmpty) {
    return null;
  }
  if (_isCborContentType(contentType)) {
    return _decodeCbor(bytes);
  }
  if (_isJsonContentType(contentType)) {
    return jsonDecode(utf8.decode(bytes));
  }
  throw FormatException(
    'Unsupported response Content-Type $contentType for $owner',
  );
}

/// Best-effort decode of an HTTP error answer's body. Never throws: a body
/// that is empty or does not decode is `null`, and a body of any other
/// `Content-Type` (a proxy's plain-text 502, say) is its text.
Future<Object?> _decodeErrorBody(Uint8List bytes, String? contentType) async {
  if (bytes.isEmpty) {
    return null;
  }
  try {
    if (_isCborContentType(contentType) || _isJsonContentType(contentType)) {
      return await _decodeBody(bytes, contentType, 'an error body');
    }
    final text = utf8.decode(bytes, allowMalformed: true).trim();
    return text.isEmpty ? null : text;
  } catch (_) {
    return null;
  }
}

/// Layers header maps, later layers winning. Header names are
/// case-insensitive, so `accept` from a later layer replaces `Accept` from an
/// earlier one instead of sitting beside it.
Map<String, String> _mergeHeaders(List<Map<String, String>?> layers) {
  final merged = <String, String>{};
  final keyByLowerName = <String, String>{};
  for (final layer in layers) {
    if (layer == null) {
      continue;
    }
    for (final entry in layer.entries) {
      final lowerName = entry.key.toLowerCase();
      final earlier = keyByLowerName[lowerName];
      if (earlier != null) {
        merged.remove(earlier);
      }
      keyByLowerName[lowerName] = entry.key;
      merged[entry.key] = entry.value;
    }
  }
  return merged;
}

/// `baseUri` + `path` + `query`, joined by concatenation rather than RFC 3986
/// resolution: a base URI with a path prefix (`https://host/v1`) keeps it, and
/// a `path` starting with `/` does not discard it. [query] is an already
/// percent-encoded query string.
Uri _resolveUri(Uri baseUri, String path, String? query) {
  final basePath = baseUri.path.endsWith('/')
      ? baseUri.path.substring(0, baseUri.path.length - 1)
      : baseUri.path;
  final tail = path.startsWith('/') ? path : '/$path';
  final origin = '${baseUri.scheme}://${baseUri.authority}';
  final suffix = (query == null || query.isEmpty) ? '' : '?$query';
  return Uri.parse('$origin$basePath$tail$suffix').normalizePath();
}

// -----------------------------------------------------------------------------
// What an error answer says beyond its code (cratestack#1208, cratestack#1243)
//
// An `AuthProvider` refusal keeps the fixed code (`UNAUTHORIZED`, RPC
// `unauthenticated`) and may add top-level members to the error body, a
// `details` member and `WWW-Authenticate` challenges. These two helpers read
// them for `CratestackRestException` and `CratestackRpcException`.
// -----------------------------------------------------------------------------

/// The members of an error body that are not `code`, `message` or `details`,
/// unmodifiable and in wire order. Empty when [body] is not a map. Nothing is
/// dropped: a member this client does not know is kept here, as decoded.
Map<String, Object?> _cratestackExtraErrorMembers(Object? body) {
  if (body is! Map) {
    return const <String, Object?>{};
  }
  const reserved = <String>{'code', 'message', 'details'};
  return Map<String, Object?>.unmodifiable(<String, Object?>{
    for (final entry in body.entries)
      if (!reserved.contains(entry.key.toString()))
        entry.key.toString(): entry.value,
  });
}

/// The `WWW-Authenticate` value of an answer's [headers], raw and unparsed,
/// as a list: empty when there is none. [CratestackHttpResponse] joins a
/// repeated header with `, `, so two challenges sent as two header lines
/// arrive as one entry holding both.
List<String> _cratestackChallenges(Map<String, String> headers) {
  for (final entry in headers.entries) {
    final value = entry.value;
    if (entry.key.toLowerCase() == 'www-authenticate' && value.isNotEmpty) {
      return List<String>.unmodifiable(<String>[value]);
    }
  }
  return const <String>[];
}

// -----------------------------------------------------------------------------
// A step-up refusal, typed (cratestack#1244)
//
// An operation that requires a fresher or stronger authentication than the
// caller presented is refused with an `AuthRefusal` whose extra member
// `error` is `insufficient_user_authentication` (RFC 9470): HTTP 401 on REST,
// code `unauthenticated` on RPC, unary or in a batch frame. Its `details` say
// what the operation requires and which requirement the caller failed.
// -----------------------------------------------------------------------------

/// The extra member `error` of a step-up refusal (RFC 9470).
const String cratestackInsufficientUserAuthentication =
    'insufficient_user_authentication';

/// What a step-up refusal asks for: read from the exception's `stepUp`. Each
/// requirement is present only when the refused operation declares it.
class StepUpRequired {
  const StepUpRequired({
    this.maxAge,
    this.acrValues = const <String>[],
    this.amr = const <String>[],
    this.unmet = const <String>[],
  });

  /// The longest time, in seconds, since the caller last authenticated.
  final int? maxAge;

  /// The authentication context classes the operation accepts.
  final List<String> acrValues;

  /// The authentication methods the operation requires.
  final List<String> amr;

  /// Which requirements the caller failed: a subset of `max_age`, `acr` and
  /// `amr`.
  final List<String> unmet;

  @override
  String toString() =>
      'StepUpRequired(maxAge=$maxAge, acrValues=$acrValues, amr=$amr, unmet=$unmet)';
}

/// Reads a step-up refusal once the transport's own test ([refused]: a 401,
/// or `unauthenticated`) passed. Never throws: a missing or mistyped field
/// reads as absent or empty.
StepUpRequired? _cratestackStepUp(
  bool refused,
  Object? details,
  Map<String, Object?> extra,
) {
  if (!refused || extra['error'] != cratestackInsufficientUserAuthentication) {
    return null;
  }
  final map = details is Map ? details : const <Object?, Object?>{};
  List<String> strings(String name) {
    final value = map[name];
    return value is List
        ? List<String>.unmodifiable(value.whereType<String>())
        : const <String>[];
  }

  final maxAge = map['max_age'];
  return StepUpRequired(
    maxAge: maxAge is int && maxAge >= 0 ? maxAge : null,
    acrValues: strings('acr_values'),
    amr: strings('amr'),
    unmet: strings('unmet'),
  );
}

/// Runs [call]; when it fails with a step-up refusal, hands the refusal to
/// [reprove] and, when that answers `true` (the user authenticated again),
/// runs [call] **once** more and returns what it gives, even a second
/// refusal. Never loops.
///
/// [reprove] answers `false` when the user declined or the re-proof failed;
/// the first refusal is then rethrown. Any other error, and an error thrown
/// by [reprove] itself, propagates as it is.
///
/// ```dart
/// final receipt = await retryAfterStepUp(
///   () => api.procedures.transfer(args),
///   (stepUp) => session.reauthenticate(maxAge: stepUp.maxAge),
/// );
/// ```
Future<T> retryAfterStepUp<T>(
  Future<T> Function() call,
  Future<bool> Function(StepUpRequired stepUp) reprove,
) async {
  try {
    return await call();
  } catch (error) {
    final stepUp = _cratestackStepUpOf(error);
    if (stepUp == null || !await reprove(stepUp)) {
      rethrow;
    }
  }
  return call();
}

typedef CratestackValueMap = Map<String, Object?>;

/// Recursively rewrites decoded containers into the `Map<String, Object?>`
/// / `List<Object?>` shapes the generated `fromWire` decoders expect.
///
/// The `cbor` package hands maps back as `Map<Object?, Object?>` (and
/// nested maps/lists likewise), which is *not* a `Map<String, dynamic>`
/// — casting it directly throws a `TypeError` (`_Map<Object?, Object?>`
/// is not a subtype of `Map<String, dynamic>`) at runtime. JSON-decoded
/// bodies already have `String` keys, so for them this is a cheap
/// structural walk.
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

Object cratestackRequireWireValue(String ownerName, String fieldName, Object? value) {
  if (value == null) {
    throw FormatException('Missing required field $ownerName.$fieldName');
  }
  return value;
}

/// Decodes a `BigInt` wire value (ADR 0019): a canonical decimal string such
/// as `"9223372036854775807"`, never a number. `BigInt.parse` is exact on the
/// VM, dart2js and dart2wasm; `int.parse` and `jsonDecode` round past 2^53 on
/// dart2js, which is why the wire form is a string at all.
///
/// A number at a `BigInt` key throws rather than being coerced: it means a
/// server from before the `BigInt` cutover, whose value may already have been
/// rounded, so accepting it would hide the corruption. [site] names the
/// `Model.field` the value was read for. A string that is not canonical
/// (`+5`, `007`, `-0`, whitespace, `0x1F`) is refused too, because
/// `BigInt.parse` would silently turn some of those into a different number.
BigInt cratestackDecodeBigInt(Object? value, String site) {
  if (value is! String) {
    throw FormatException(
      '$site: expected a BigInt as a canonical decimal string, '
      'got ${value.runtimeType} ($value)',
    );
  }
  if (!_cratestackCanonicalBigInt.hasMatch(value)) {
    throw FormatException(
      '$site: expected a BigInt as a canonical decimal string',
      value,
    );
  }
  return BigInt.parse(value);
}

final RegExp _cratestackCanonicalBigInt = RegExp(r'^(0|-?[1-9][0-9]*)$');

/// Folds a typed computed-params class's already-`.toWire()`'d map
/// (same call-site convention as `rest-runtime.dart.j2`'s
/// `cratestackWithComputedParams`) into an RPC `model.<X>.get`/
/// `model.<X>.list` input frame under the `computedParams` key, encoded
/// as raw JSON text rather than left as a nested object
/// (`docs/design/computed-fields.md`'s "Why `computedParams` is a
/// `String`" section: `/rpc/batch` re-encodes each frame's opaque
/// `input` through `serde_json::Value` before re-dispatching it, and a
/// nested `Option`-bearing object doesn't survive that round trip —
/// a `String` field does). Absent/empty `computedParams` leaves `input`
/// untouched — never adds an empty `computedParams` key.
Map<String, Object?> cratestackWithRpcComputedParams(
  Map<String, Object?> input,
  Map<String, Object?>? computedParams,
) {
  if (computedParams == null || computedParams.isEmpty) {
    return input;
  }
  return <String, Object?>{
    ...input,
    'computedParams': jsonEncode(computedParams),
  };
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
  /// An `@isolation` procedure's transaction was rolled back after
  /// repeated concurrent-update conflicts. Nothing was committed; sending
  /// the same call again (even under the same `Idempotency-Key`) is
  /// expected to succeed. Unlike [conflict], which a retry repeats.
  static const String aborted = 'aborted';
  static const String failedPrecondition = 'failed_precondition';
  static const String resourceExhausted = 'resource_exhausted';
  static const String unavailable = 'unavailable';
  static const String deadlineExceeded = 'deadline_exceeded';
  static const String canceled = 'canceled';
  static const String internal = 'internal';
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

/// Wire shape of an RPC error body. Mirrors
/// `cratestack_core::rpc::RpcErrorBody`.
class RpcErrorBody {
  const RpcErrorBody({
    required this.code,
    required this.message,
    this.details,
    this.extra = const <String, Object?>{},
  });

  final String code;
  final String message;
  final Object? details;

  /// Every top-level member of the body other than `code`, `message` and
  /// `details`, as decoded: what an `AuthProvider` refusal adds, such as
  /// `{'error': 'attestation_expired'}` (cratestack#1208). Empty when the
  /// server sent none; a member this client does not know is kept, never
  /// dropped.
  final Map<String, Object?> extra;

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
      extra: _cratestackExtraErrorMembers(map),
    );
  }
}

/// Thrown by an RPC adapter when a remote call fails with an
/// `RpcErrorBody` payload. Carries the gRPC-style `code` directly so
/// callers can switch on it without parsing.
///
/// A refusal from the server's `AuthProvider` keeps its fixed code
/// (`unauthenticated` or `permission_denied`); what it adds is in [details],
/// [extra] and [challenges]:
///
/// ```dart
/// on CratestackRpcException catch (e) {
///   if (e.code == CratestackRpcErrorCodes.unauthenticated &&
///       e.extra['error'] == 'attestation_expired') {
///     // renew the credential and send again, once
///   }
/// }
/// ```
class CratestackRpcException implements Exception {
  CratestackRpcException({
    required this.status,
    required this.body,
    this.headers = const <String, String>{},
  });

  final int status;
  final RpcErrorBody body;

  /// The answer's headers, names lower-cased (a repeated header joined with
  /// `, `). Empty for an error that arrived inside a batch frame or as a
  /// stream's final item, which have no headers of their own.
  final Map<String, String> headers;

  String get code => body.code;
  String get message => body.message;
  Object? get details => body.details;

  /// [RpcErrorBody.extra]: the body's members beyond `code`, `message` and
  /// `details`.
  Map<String, Object?> get extra => body.extra;

  /// The answer's `WWW-Authenticate` value, raw and unparsed; empty when
  /// there is none. A transport joins two challenge lines into one entry.
  List<String> get challenges => _cratestackChallenges(headers);

  /// The step-up refusal this answer is (cratestack#1244): code
  /// `unauthenticated` and the [extra] member `error` set to
  /// `insufficient_user_authentication`, read from [details]; a batch frame
  /// reads the same. `null` for any other error; reading never throws.
  StepUpRequired? get stepUp => _cratestackStepUp(
        code == CratestackRpcErrorCodes.unauthenticated,
        details,
        extra,
      );

  @override
  String toString() =>
      'CratestackRpcException(status=$status, code=${body.code}, message=${body.message})';
}

/// What [retryAfterStepUp] reads: the step-up refusal of a
/// [CratestackRpcException], `null` for anything else.
StepUpRequired? _cratestackStepUpOf(Object error) =>
    error is CratestackRpcException ? error.stepUp : null;

/// Per-call settings for the RPC adapter.
class CratestackRpcCallOptions {
  const CratestackRpcCallOptions({
    this.headers = const <String, String>{},
    this.idempotencyKey,
    this.abort,
  });

  /// Extra request headers; they win over the adapter's own.
  final Map<String, String> headers;

  /// Sent as `Idempotency-Key`, so a retried mutation is applied once.
  final String? idempotencyKey;

  /// Completing this future cancels the call; the adapter then throws a
  /// [CratestackTransportAborted].
  final Future<void>? abort;
}

/// Adapter interface — implementations carry the actual transport
/// (HTTP through a [CratestackHttpTransport], FFI to the Rust runtime,
/// in-process stub, etc.) and the codec choice. The generated API classes
/// only know about this interface; swapping JSON for CBOR is a constructor
/// flip.
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

bool _isCborSeqContentType(String? contentType) =>
    _cratestackMimeOf(contentType) == 'application/cbor-seq';

/// The RPC adapter, written once over a [CratestackHttpTransport].
///
/// It owns the headers, the CBOR/JSON choice and the dispatch on the answer's
/// `Content-Type`, and the error mapping: an HTTP error answer becomes a
/// [CratestackRpcException], a call that got no answer at all fails with a
/// [CratestackTransportException]. The transport owns the connection. To use
/// an HTTP client of your own, implement [CratestackHttpTransport]; to use
/// fespalier's transport chain, wrap it:
///
/// ```dart
/// final adapter = CratestackRpcHttpAdapter(
///   CratestackFespalierTransport(transportChain),
///   baseUri: Uri.parse('https://api.example.com'),
/// );
/// ```
class CratestackRpcHttpAdapter implements CratestackRpcAdapter {
  const CratestackRpcHttpAdapter(
    this.transport, {
    required this.baseUri,
    this.format = CratestackWireFormat.cbor,
    this.cborSeqDecoder,
  });

  final CratestackHttpTransport transport;

  /// Scheme, authority and optional path prefix `/rpc/...` is joined onto:
  /// `https://api.example.com` or `https://example.com/v1`.
  final Uri baseUri;

  /// The wire format of request bodies and the `Accept` preference.
  final CratestackWireFormat format;

  /// Makes the scanner that splits an `application/cbor-seq` answer into
  /// items, one per `stream()` call. Defaults to [CratestackCborSeqScanner],
  /// which is pure Dart.
  final CborSeqDecoderHandle Function()? cborSeqDecoder;

  @override
  Future<Object?> call(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async {
    final response = await _post(
      '/rpc/${Uri.encodeComponent(opId)}',
      input,
      accept: _acceptForUnary,
      options: options,
      idempotent: true,
    );
    return _decodeSuccess(response);
  }

  @override
  Future<List<RpcResponseFrame>> batch(
    List<RpcRequest> requests, {
    CratestackRpcCallOptions? options,
  }) async {
    final response = await _post(
      '/rpc/batch',
      requests.map((r) => r.toWire()).toList(growable: false),
      accept: _acceptForUnary,
      options: options,
    );
    final frames = await _decodeSuccess(response) as List<dynamic>;
    return frames
        .map((frame) => RpcResponseFrame.fromWire(frame))
        .toList(growable: false);
  }

  @override
  Stream<Object?> stream(
    String opId,
    Object? input, {
    CratestackRpcCallOptions? options,
  }) async* {
    final response = await _post(
      '/rpc/${Uri.encodeComponent(opId)}',
      input,
      accept: format == CratestackWireFormat.json
          ? 'application/json, application/cbor-seq;q=0.9'
          : 'application/cbor-seq, application/cbor, application/json;q=0.9',
      options: options,
    );
    if (response.status >= 200 && response.status < 300 &&
        _isCborSeqContentType(response.headers['content-type'])) {
      // Frame by frame: items are yielded as their bytes arrive, the body is
      // never buffered.
      final decoder = (cborSeqDecoder ?? CratestackCborSeqScanner.new)();
      await for (final item in response.body.transform(
        CborSeqStreamTransformer(decoder),
      )) {
        final tag = _cratestackReadLeadingTag(item);
        if (tag != null && tag.$1 == cratestackRpcStreamErrorTag) {
          // The stream-error sentinel is always the last item.
          final error = await _decodeBody(
            Uint8List.sublistView(item, tag.$2),
            'application/cbor',
            'CratestackRpcHttpAdapter',
          );
          throw CratestackRpcException(
            status: response.status,
            body: RpcErrorBody.fromWire(error),
          );
        }
        yield await _decodeBody(item, 'application/cbor', 'CratestackRpcHttpAdapter');
      }
      return;
    }
    // The server answered with one array (`application/cbor` or JSON).
    final decoded = await _decodeSuccess(response);
    if (decoded == null) {
      return;
    }
    for (final item in decoded as List<dynamic>) {
      yield item;
    }
  }

  // Mirrors the retired JSON adapter, which sent no `Accept` on a unary call
  // or a batch; the CBOR format states its preference.
  String? get _acceptForUnary => format == CratestackWireFormat.json
      ? null
      : 'application/cbor, application/json;q=0.9';

  Future<CratestackHttpResponse> _post(
    String path,
    Object? body, {
    required String? accept,
    required CratestackRpcCallOptions? options,
    bool idempotent = false,
  }) async {
    final encoded = await _encodeBody(format, body);
    return transport.send(
      CratestackHttpRequest(
        method: 'POST',
        uri: _resolveUri(baseUri, path, null),
        headers: _mergeHeaders([
          <String, String>{
            if (accept != null) 'Accept': accept,
            'Content-Type': format == CratestackWireFormat.json
                ? 'application/json'
                : 'application/cbor',
            if (cratestackSchemaSha256 != null)
              'x-cratestack-schema-sha': cratestackSchemaSha256!,
            if (idempotent && options?.idempotencyKey != null)
              'Idempotency-Key': options!.idempotencyKey!,
          },
          options?.headers,
        ]),
        body: encoded,
        abort: options?.abort,
      ),
    );
  }

  /// Reads a unary answer: the decoded body on 2xx, a
  /// [CratestackRpcException] otherwise.
  Future<Object?> _decodeSuccess(CratestackHttpResponse response) async {
    final bytes = await response.bytes();
    final contentType = response.headers['content-type'];
    if (response.status < 200 || response.status >= 300) {
      final data = await _decodeErrorBody(bytes, contentType);
      throw CratestackRpcException(
        status: response.status,
        body: RpcErrorBody.fromWire(
          data ?? <String, Object?>{
            'code': CratestackRpcErrorCodes.internal,
            'message': 'HTTP ${response.status}',
          },
        ),
        headers: response.headers,
      );
    }
    return _decodeBody(bytes, contentType, 'CratestackRpcHttpAdapter');
  }
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

// -----------------------------------------------------------------------------
// Pure-Dart `application/cbor-seq` boundary scanner
//
// The default `CborSeqDecoderHandle` for `CratestackRpcHttpAdapter.stream()`:
// no FFI, so it works wherever the generated package does. A host with a
// faster scanner (an FFI-backed one from `cratestack-client-flutter`) passes
// its own `CborSeqDecoderHandle` as `cborSeqDecoder:` instead.
//
// A structural walk only: it finds where each top-level item ends, handling
// every major type including indefinite-length strings, arrays and maps
// (RFC 8949 §3.2). A port of the TypeScript client's `CborSeqBoundaryScanner`.
// -----------------------------------------------------------------------------

/// `Tag(48900, RpcErrorBody)` as a cbor-seq stream's final item marks a
/// failure mid-stream (`cratestack_core::rpc::RPC_STREAM_ERROR_TAG`,
/// `docs/design/rpc-transport.md` §3.3).
const int cratestackRpcStreamErrorTag = 48900;

class CratestackCborSeqScanner implements CborSeqDecoderHandle {
  Uint8List _buffer = Uint8List(0);

  @override
  Future<List<Uint8List>> feed(Uint8List chunk) {
    if (_buffer.isEmpty) {
      _buffer = chunk;
    } else {
      final joined = Uint8List(_buffer.length + chunk.length)
        ..setRange(0, _buffer.length, _buffer)
        ..setRange(_buffer.length, _buffer.length + chunk.length, chunk);
      _buffer = joined;
    }
    final items = <Uint8List>[];
    var consumed = 0;
    while (consumed < _buffer.length) {
      final end = _cratestackSkipCborItem(_buffer, consumed, 0);
      if (end == null) {
        break;
      }
      items.add(Uint8List.fromList(_buffer.sublist(consumed, end)));
      consumed = end;
    }
    if (consumed > 0) {
      _buffer = Uint8List.fromList(_buffer.sublist(consumed));
    }
    return Future<List<Uint8List>>.value(items);
  }

  @override
  int pendingLen() => _buffer.length;
}

const int _cratestackCborMaxDepth = 256;

/// The offset just past the item starting at [offset], or `null` when more
/// bytes are needed. Throws a [FormatException] for bytes no further data can
/// make valid.
int? _cratestackSkipCborItem(Uint8List bytes, int offset, int depth) {
  if (depth > _cratestackCborMaxDepth) {
    throw const FormatException('cbor-seq item is nested too deeply');
  }
  if (offset >= bytes.length) {
    return null;
  }
  final initial = bytes[offset];
  final major = initial >> 5;
  final info = initial & 0x1f;
  final cursor = offset + 1;

  if (major == 7) {
    return _cratestackSkipSimple(bytes, cursor, info);
  }
  if (info == 31) {
    if (major == 0 || major == 1 || major == 6) {
      throw FormatException('major type $major cannot be indefinite-length');
    }
    return _cratestackSkipIndefinite(bytes, cursor, major, depth);
  }
  final argument = _cratestackReadArgument(bytes, cursor, info);
  if (argument == null) {
    return null;
  }
  final (value, next) = argument;
  switch (major) {
    case 0:
    case 1:
      return next;
    case 2:
    case 3:
      final end = next + value;
      return end > bytes.length ? null : end;
    case 4:
      return _cratestackSkipItems(bytes, next, value, depth);
    case 5:
      return _cratestackSkipItems(bytes, next, value * 2, depth);
    default:
      return _cratestackSkipCborItem(bytes, next, depth + 1);
  }
}

int? _cratestackSkipItems(Uint8List bytes, int start, int count, int depth) {
  var cursor = start;
  for (var i = 0; i < count; i++) {
    final end = _cratestackSkipCborItem(bytes, cursor, depth + 1);
    if (end == null) {
      return null;
    }
    cursor = end;
  }
  return cursor;
}

int? _cratestackSkipIndefinite(Uint8List bytes, int start, int major, int depth) {
  var cursor = start;
  while (true) {
    if (cursor >= bytes.length) {
      return null;
    }
    if (bytes[cursor] == 0xff) {
      return cursor + 1;
    }
    if (major == 2 || major == 3) {
      // RFC 8949 §3.2.3: chunks are definite-length strings of the same type.
      if (bytes[cursor] >> 5 != major || (bytes[cursor] & 0x1f) == 31) {
        throw const FormatException('malformed indefinite-length cbor string');
      }
    }
    final end = _cratestackSkipCborItem(bytes, cursor, depth + 1);
    if (end == null) {
      return null;
    }
    cursor = end;
    if (major == 5) {
      final valueEnd = _cratestackSkipCborItem(bytes, cursor, depth + 1);
      if (valueEnd == null) {
        return null;
      }
      cursor = valueEnd;
    }
  }
}

int? _cratestackSkipSimple(Uint8List bytes, int cursor, int info) {
  final int width;
  if (info <= 23) {
    return cursor;
  } else if (info == 24) {
    width = 1;
  } else if (info == 25) {
    width = 2;
  } else if (info == 26) {
    width = 4;
  } else if (info == 27) {
    width = 8;
  } else if (info == 31) {
    throw const FormatException('unexpected cbor break code');
  } else {
    throw FormatException('reserved cbor additional info $info');
  }
  return cursor + width > bytes.length ? null : cursor + width;
}

/// `(value, offsetAfterArgument)`, or `null` when the argument's bytes have
/// not all arrived.
(int, int)? _cratestackReadArgument(Uint8List bytes, int cursor, int info) {
  if (info <= 23) {
    return (info, cursor);
  }
  final width = switch (info) {
    24 => 1,
    25 => 2,
    26 => 4,
    27 => 8,
    _ => throw FormatException('reserved cbor additional info $info'),
  };
  if (cursor + width > bytes.length) {
    return null;
  }
  // Past 2^53 a length is not exactly representable on every platform, and no
  // real frame is that large.
  if (width == 8 && (bytes[cursor] != 0 || bytes[cursor + 1] > 0x1f)) {
    throw const FormatException('cbor-seq item length exceeds what is supported');
  }
  var value = 0;
  for (var i = 0; i < width; i++) {
    value = value * 256 + bytes[cursor + i];
  }
  return (value, cursor + width);
}

/// `(tagNumber, headerLength)` when [item] starts with a CBOR tag, else
/// `null`. Reads only the tag header, never the tagged content.
(int, int)? _cratestackReadLeadingTag(Uint8List item) {
  if (item.isEmpty || item[0] >> 5 != 6) {
    return null;
  }
  try {
    return _cratestackReadArgument(item, 1, item[0] & 0x1f);
  } on FormatException {
    return null;
  }
}
