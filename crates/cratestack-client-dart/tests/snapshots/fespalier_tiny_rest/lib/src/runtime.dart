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

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    final body = await _encodeBody(format, request.body);
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
                : 'application/cbor, application/json;q=0.9',
            if (body != null)
              'Content-Type': format == CratestackWireFormat.json
                  ? 'application/json'
                  : 'application/cbor',
            if (cratestackSchemaSha256 != null)
              'x-cratestack-schema-sha': cratestackSchemaSha256!,
            if (options?.idempotencyKey != null)
              'Idempotency-Key': options!.idempotencyKey!,
          },
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

    final bytes = await response.bytes();
    final contentType = response.headers['content-type'];
    if (response.status < 200 || response.status >= 300) {
      throw CratestackRestException(
        status: response.status,
        body: await _decodeErrorBody(bytes, contentType),
        headers: response.headers,
      );
    }
    return _decodeBody(bytes, contentType, 'CratestackHttpAdapter');
  }
}
