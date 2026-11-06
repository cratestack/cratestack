import 'dart:convert';
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart' as cratestack_cbor;

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

// cratestack#563: `createCborCodec()` is async (it loads a
// vendored native library / wasm module on first use), so the codec is
// resolved once and cached here rather than per call — every
// `_cratestackCborCodec()` caller after the first just awaits the same
// already-in-flight (or already-resolved) `Future`.
//
// cratestack#798: only a SUCCESSFUL resolution stays cached. A plain
// `??=` never re-evaluates once the field holds a settled *rejected*
// future, so one transient failure — a wasm asset that 404s on web, a
// vendored library that was not there yet — would brick every later
// request in the isolate, replaying the same error forever instead of
// giving the next caller a fresh attempt. `onError` clears the cache so
// it can retry, and rethrows with the original stack trace so nothing is
// swallowed. Same rule, for the same reason, as the generated TypeScript
// RPC runtime's `resolveCodec()` and `cratestack_cbor`'s own
// `createCborCodec()`.
Future<cratestack_cbor.CratestackCborCodec>? _cratestackCborCodecFuture;

Future<cratestack_cbor.CratestackCborCodec> _cratestackCborCodec() {
  return _cratestackCborCodecFuture ??= cratestack_cbor
      .createCborCodec()
      .onError<Object>((error, stackTrace) {
        // Hung off the future rather than written as a `try`/`catch`
        // around the call so it cannot run before the `??=` has
        // assigned: an `onError` callback is always asynchronous,
        // whereas a `catch` body fires synchronously for anything thrown
        // before the factory's first `await`, and the assignment would
        // then immediately re-cache the very failure it just cleared.
        _cratestackCborCodecFuture = null;
        Error.throwWithStackTrace(error, stackTrace);
      });
}

/// The wire format a request body is written in and an `Accept` header
/// prefers. Responses are always decoded by their own `Content-Type`, so an
/// adapter set to one format still reads the other.
enum CratestackWireFormat {
  /// `application/cbor`, the framework's default wire format.
  cbor,

  /// `application/json`.
  json,
}

/// The `Accept` of a CBOR adapter: CBOR preferred, JSON accepted.
const String _cratestackPlainCborAccept = 'application/cbor, application/json;q=0.9';

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
      final codec = await _cratestackCborCodec();
      return codec.encodeJson(jsonEncode(body));
  }
}

/// Decodes a CBOR body to the JSON-plain value graph the generated `fromWire`
/// decoders expect.
Future<Object?> _decodeCbor(Uint8List bytes) async {
  // `cratestack_cbor`'s wire boundary is JSON text, and every generated
  // `toWire()`/`fromWire` already produces/expects JSON-plain values
  // (ISO-8601 strings for `DateTime`, `List<int>` for `Bytes`, decimal and
  // `BigInt` text), so `jsonDecode` alone gives the `Map<String, Object?>`
  // shape the decoders expect.
  final codec = await _cratestackCborCodec();
  return jsonDecode(codec.decodeJson(bytes));
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
// Compact wire keys (opt-in)
//
// With `compact:` hooks handed to an adapter, a CBOR call advertises that it
// can read integer-keyed bodies (`Accept: <compact type>, <plain types>` plus
// `Cratestack-Contract`, the selector of this client's contract for the op),
// turns a compact response back into the plain bytes the normal decoder
// reads, and, only once that op has answered compact, sends its own request
// body compact too, so an older server is never sent a body it cannot read.
// Without hooks (`compact: null`, the default) none of this runs and the wire
// is the one the adapter always spoke. JSON stays named; so do error bodies
// and `/rpc/batch` frames.
//
// The hooks are plain functions of bytes and this runtime depends on no
// CrateStack API for them. An app builds them from `cratestackKeyTables`:
//
//   final keys = await createCompactKeys(cratestackKeyTables);
//   final hooks = CratestackCompactHooks(
//     toCompact: keys.toCompact,
//     fromCompact: keys.fromCompact,
//     selector: keys.selector,
//   );
// -----------------------------------------------------------------------------

/// Rewrites a CBOR body between its named and its compact (integer-keyed)
/// form for one op. [direction] is `'request'` or `'response'`.
typedef CratestackCompactBytes = Uint8List Function(
  Uint8List bytes,
  String op,
  String direction,
);

/// The `Cratestack-Contract` header value for [op]'s current contract, or
/// `null` when the key tables do not know the op (it then stays named).
typedef CratestackCompactSelector = String? Function(String op);

/// The three functions that put an adapter on compact wire keys.
class CratestackCompactHooks {
  const CratestackCompactHooks({
    required this.toCompact,
    required this.fromCompact,
    required this.selector,
  });

  /// Named CBOR to compact CBOR (a request body).
  final CratestackCompactBytes toCompact;

  /// Compact CBOR to named CBOR (a response body or one `cbor-seq` item).
  final CratestackCompactBytes fromCompact;

  /// The `Cratestack-Contract` value for an op, or `null` to stay named.
  final CratestackCompactSelector selector;
}

const String _cratestackCompactCbor = 'application/vnd.cratestack.compact+cbor';
const String _cratestackCompactCborSeq =
    'application/vnd.cratestack.compact+cbor-seq';
const int _cratestackRememberedEncodings = 512;

/// What one adapter has learned: which ops answered compact, and the encoding
/// each `Idempotency-Key` was first sent in. Held per adapter (the adapters
/// are `const`, so it lives in an [Expando], not a field).
final class _CompactMemory {
  final Set<String> compactOps = <String>{};
  final Map<String, bool> encodingByIdempotencyKey = <String, bool>{};
}

final Expando<_CompactMemory> _cratestackCompactMemory =
    Expando<_CompactMemory>('cratestackCompactMemory');

/// The compact plan of one call.
final class _CompactCall {
  _CompactCall._(
    this._hooks,
    this._memory,
    this.op,
    this._selector,
    this._sendCompact,
    this.stream,
  );

  final CratestackCompactHooks _hooks;
  final _CompactMemory _memory;
  final String op;
  final String _selector;
  final bool _sendCompact;

  /// A `cbor-seq` call: its response media type is the compact sequence.
  final bool stream;

  /// The compact media type a response must carry to be transcoded.
  String get responseType =>
      stream ? _cratestackCompactCborSeq : _cratestackCompactCbor;

  /// `Accept` with the compact type ahead of [plain], the adapter's own list.
  String accept(String plain) => '$responseType, $plain';

  /// `Cratestack-Contract`, and the compact `Content-Type` once this op has
  /// answered compact and the call has a body.
  Map<String, String> headers({required bool hasBody}) => <String, String>{
    'Cratestack-Contract': _selector,
    if (_sendCompact && hasBody) 'Content-Type': _cratestackCompactCbor,
  };

  /// The request body, transcoded when this op already answered compact.
  Uint8List? encode(Uint8List? body) {
    if (!_sendCompact || body == null) {
      return body;
    }
    return _hooks.toCompact(body, op, 'request');
  }

  /// Whether a response with [contentType] is compact.
  bool matches(String? contentType) =>
      _cratestackMimeOf(contentType) == responseType;

  /// A compact response body (or one `cbor-seq` item) as plain CBOR; records
  /// that this op answers compact.
  Uint8List decode(Uint8List bytes) {
    _memory.compactOps.add(op);
    return _hooks.fromCompact(bytes, op, 'response');
  }
}

/// The compact plan for one call of [owner], or `null` to stay named: no
/// hooks, the JSON format, or an op the hooks' tables do not know.
_CompactCall? _cratestackCompactBegin(
  Object owner,
  CratestackCompactHooks? hooks,
  CratestackWireFormat format,
  String op, {
  bool stream = false,
  String? idempotencyKey,
}) {
  if (hooks == null || format != CratestackWireFormat.cbor) {
    return null;
  }
  final selector = hooks.selector(op);
  if (selector == null) {
    return null;
  }
  final memory = _cratestackCompactMemory[owner] ??= _CompactMemory();
  // A retry under the same `Idempotency-Key` reuses the first attempt's encoding.
  final remembered = idempotencyKey == null
      ? null
      : memory.encodingByIdempotencyKey[idempotencyKey];
  final sendCompact = remembered ?? memory.compactOps.contains(op);
  if (idempotencyKey != null && remembered == null) {
    memory.encodingByIdempotencyKey[idempotencyKey] = sendCompact;
    if (memory.encodingByIdempotencyKey.length > _cratestackRememberedEncodings) {
      memory.encodingByIdempotencyKey.remove(
        memory.encodingByIdempotencyKey.keys.first,
      );
    }
  }
  return _CompactCall._(hooks, memory, op, selector, sendCompact, stream);
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

/// Folds a `computedParams` map (keyed by `@computed` field name, each
/// value the field's params serialized as plain JSON-compatible Dart
/// objects — `docs/design/computed-fields.md`) into a request's other
/// query parameters, under the single wire query key `computedParams`, as
/// a JSON-encoded string. [cratestackCanonicalizeQuery] percent-encodes
/// map values itself, so nothing here needs to pre-escape the JSON text.
/// Absent/empty `computedParams` leaves `queryParameters` untouched
/// (never adds an empty `computedParams=` key) — the server treats a
/// missing key exactly like `None` for every computed field.
Map<String, Object?>? cratestackWithComputedParams(
  Map<String, Object?>? queryParameters,
  Map<String, Object?>? computedParams,
) {
  if (computedParams == null || computedParams.isEmpty) {
    return queryParameters;
  }
  return <String, Object?>{
    ...?queryParameters,
    'computedParams': jsonEncode(computedParams),
  };
}

/// The canonical query string of a request: `key=value` pairs joined by `&`,
/// keys and values percent-encoded with [Uri.encodeQueryComponent], in map
/// order. It does not depend on any HTTP client's own list or null rules:
///
///  * a `null` value is omitted (never a bare `key`);
///  * a list repeats the key, `tag=a&tag=b`;
///  * a nested map nests with literal brackets, `filter[kind]=x`;
///  * every other value is its `toString()`.
///
/// Returns `null` when there is nothing to send. For the string, int and bool
/// values the generated query builders produce, the output is byte-identical
/// to what the retired Dio adapters sent (pinned by `wire_parity_rest_test`).
String? cratestackCanonicalizeQuery(Map<String, Object?>? queryParameters) {
  if (queryParameters == null || queryParameters.isEmpty) {
    return null;
  }
  final pairs = <String>[];
  void add(String encodedKey, Object? value) {
    if (value == null) {
      return;
    }
    if (value is Map) {
      value.forEach((key, entry) {
        add('$encodedKey[${Uri.encodeQueryComponent(key.toString())}]', entry);
      });
      return;
    }
    if (value is Iterable) {
      for (final item in value) {
        add(encodedKey, item);
      }
      return;
    }
    pairs.add('$encodedKey=${Uri.encodeQueryComponent(value.toString())}');
  }

  queryParameters.forEach((key, value) => add(Uri.encodeQueryComponent(key), value));
  return pairs.isEmpty ? null : pairs.join('&');
}

/// Per-call settings for the REST adapter.
class CratestackCallOptions {
  const CratestackCallOptions({
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

/// Thrown by [CratestackHttpAdapter] when the server answers with a non-2xx
/// status. [body] is the decoded error body (CBOR or JSON), the text of a body
/// of any other `Content-Type`, or `null` when there was none or it did not
/// decode.
///
/// A call that gets no answer at all fails with a
/// [CratestackTransportException] instead, so `on CratestackRestException`
/// means "the server said no" and nothing else.
///
/// The envelope's members are typed by the getters: [code], [message],
/// [details], [extra] (every other member, as an `AuthProvider` refusal adds
/// them, cratestack#1208) and [challenges] (the `WWW-Authenticate` value).
/// [body] still holds the whole decoded body.
class CratestackRestException implements Exception {
  const CratestackRestException({
    required this.status,
    this.body,
    this.headers = const <String, String>{},
  });

  final int status;
  final Object? body;

  /// The answer's headers, names lower-cased (a repeated header joined with
  /// `, `).
  final Map<String, String> headers;

  /// The envelope's `code` (`UNAUTHORIZED`, `FORBIDDEN`, ...), or `null` when
  /// [body] is not an envelope.
  String? get code => _envelopeMember('code') as String?;

  /// The envelope's `message`, or `null` when [body] is not an envelope.
  String? get message => _envelopeMember('message') as String?;

  /// The envelope's `details`, as decoded.
  Object? get details => _envelopeMember('details');

  /// Every top-level member of the envelope other than `code`, `message` and
  /// `details`, as decoded; empty when there is none or [body] is not a map.
  Map<String, Object?> get extra => _cratestackExtraErrorMembers(body);

  /// The answer's `WWW-Authenticate` value, raw and unparsed; empty when
  /// there is none. A transport joins two challenge lines into one entry.
  List<String> get challenges => _cratestackChallenges(headers);

  /// The step-up refusal this answer is (cratestack#1244): a 401 whose
  /// [extra] member `error` is `insufficient_user_authentication`, read from
  /// [details]. `null` for any other error; reading never throws.
  StepUpRequired? get stepUp => _cratestackStepUp(status == 401, details, extra);

  Object? _envelopeMember(String name) {
    final body = this.body;
    if (body is! Map) {
      return null;
    }
    final value = body[name];
    return name == 'details' || value is String ? value : null;
  }

  @override
  String toString() => 'CratestackRestException(status=$status, body=$body)';
}

/// What [retryAfterStepUp] reads: the step-up refusal of a
/// [CratestackRestException], `null` for anything else.
StepUpRequired? _cratestackStepUpOf(Object error) =>
    error is CratestackRestException ? error.stepUp : null;

/// One `<METHOD> <route template>` key of [cratestackOpContracts], split for
/// matching a concrete request path against it.
final class _RouteTemplate {
  _RouteTemplate(this.op)
    : method = op.substring(0, op.indexOf(' ')),
      segments = op.substring(op.indexOf(' ') + 1).split('/'),
      literals = op
          .substring(op.indexOf(' ') + 1)
          .split('/')
          .where((segment) => !segment.startsWith('{'))
          .length;

  final String op;
  final String method;
  final List<String> segments;
  final int literals;

  /// Whether [path] ends in this template. Request paths carry the client's
  /// base path (`/api` by default) in front of the route, which the keys of
  /// [cratestackOpContracts] do not, so the template is matched against the
  /// trailing segments.
  bool matches(String requestMethod, List<String> path) {
    if (method != requestMethod || path.length < segments.length) {
      return false;
    }
    final offset = path.length - segments.length;
    for (var index = 0; index < segments.length; index += 1) {
      final part = segments[index];
      if (!part.startsWith('{') && part != path[offset + index]) {
        return false;
      }
    }
    return true;
  }
}

final List<_RouteTemplate> _cratestackRouteTemplates = <_RouteTemplate>[
  for (final op in cratestackOpContracts.keys)
    if (op.contains(' ')) _RouteTemplate(op),
];

/// The op key (`GET /widgets/{id}`) a request is, or `null` for a route the
/// schema does not have. The template with the most literal segments wins
/// (`/widgets/search` over `/widgets/{id}`).
String? _cratestackRestOp(String method, String path) {
  final cut = path.indexOf('?');
  final segments = (cut < 0 ? path : path.substring(0, cut)).split('/');
  while (segments.isNotEmpty && segments.last.isEmpty) {
    segments.removeLast();
  }
  final verb = method.toUpperCase() == 'HEAD' ? 'GET' : method.toUpperCase();
  _RouteTemplate? best;
  for (final template in _cratestackRouteTemplates) {
    if (template.matches(verb, segments) &&
        (best == null ||
            template.segments.length > best.segments.length ||
            (template.segments.length == best.segments.length &&
                template.literals > best.literals))) {
      best = template;
    }
  }
  return best?.op;
}

/// The REST adapter, written once over a [CratestackHttpTransport].
///
/// It owns the headers, the CBOR/JSON choice and the dispatch on the answer's
/// `Content-Type`, the canonical query string, and the error mapping; the
/// transport owns the connection. To use an HTTP client of your own,
/// implement [CratestackHttpTransport]; to use fespalier's transport
/// chain, wrap it:
///
/// ```dart
/// final adapter = CratestackHttpAdapter(
///   CratestackFespalierTransport(transportChain),
///   baseUri: Uri.parse('https://api.example.com'),
/// );
/// ```
class CratestackHttpAdapter implements CratestackClientAdapter {
  const CratestackHttpAdapter(
    this.transport, {
    required this.baseUri,
    this.format = CratestackWireFormat.cbor,
    this.useRustTransport = false,
    this.compact,
  });

  final CratestackHttpTransport transport;

  /// Scheme, authority and optional path prefix every request path is joined
  /// onto: `https://api.example.com` or `https://example.com/v1`.
  final Uri baseUri;

  /// The wire format of request bodies and the `Accept` preference.
  final CratestackWireFormat format;

  /// Marks every request with [cratestackUseRustTransportExtraKey] in
  /// [CratestackHttpRequest.extra] (never on the wire), for a host
  /// interceptor that routes marked requests through the Rust runtime.
  final bool useRustTransport;

  /// Opts the adapter into compact wire keys (CBOR only; see
  /// [CratestackCompactHooks]). `null`, the default, keeps the wire exactly
  /// as it always was.
  final CratestackCompactHooks? compact;

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    final plainBody = await _encodeBody(format, request.body);
    final op = compact == null ? null : _cratestackRestOp(request.method, request.path);
    final plan = op == null
        ? null
        : _cratestackCompactBegin(
            this,
            compact,
            format,
            op,
            idempotencyKey: options?.idempotencyKey,
          );
    final body = plan?.encode(plainBody) ?? plainBody;
    final response = await transport.send(
      CratestackHttpRequest(
        method: request.method,
        uri: _resolveUri(
          baseUri,
          request.path,
          cratestackCanonicalizeQuery(request.queryParameters),
        ),
        headers: _mergeHeaders([
          <String, String>{
            // Explicit, never left to the HTTP stack's default (typically
            // `*/*`, sometimes nothing): a server exposing more than one wire
            // codec may resolve an absent or wildcard `Accept` to something
            // other than what this adapter asked for, and a router with only
            // a JSON codec can fail a request that did not name JSON
            // (cratestack#303).
            'Accept': format == CratestackWireFormat.json
                ? 'application/json'
                : plan?.accept(_cratestackPlainCborAccept) ??
                    _cratestackPlainCborAccept,
            if (body != null)
              'Content-Type': format == CratestackWireFormat.json
                  ? 'application/json'
                  : 'application/cbor',
            if (cratestackSchemaSha256 != null)
              'x-cratestack-schema-sha': cratestackSchemaSha256!,
            if (options?.idempotencyKey != null)
              'Idempotency-Key': options!.idempotencyKey!,
          },
          plan?.headers(hasBody: body != null),
          request.headers,
          options?.headers,
        ]),
        body: body,
        abort: options?.abort,
        extra: <String, Object?>{
          if (useRustTransport) cratestackUseRustTransportExtraKey: true,
        },
      ),
    );

    var bytes = await response.bytes();
    var contentType = response.headers['content-type'];
    if (response.status < 200 || response.status >= 300) {
      throw CratestackRestException(
        status: response.status,
        body: await _decodeErrorBody(bytes, contentType),
        headers: response.headers,
      );
    }
    if (plan != null && bytes.isNotEmpty && plan.matches(contentType)) {
      bytes = plan.decode(bytes);
      contentType = 'application/cbor';
    }
    return _decodeBody(bytes, contentType, 'CratestackHttpAdapter');
  }
}
