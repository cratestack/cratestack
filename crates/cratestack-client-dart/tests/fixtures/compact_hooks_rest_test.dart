// Compact wire keys over REST: the adapter's `compact:` hooks, driven through
// a scripted transport with a reversible stand-in for `createCompactKeys` (see
// `_StandInKeys`). Covers the negotiation (`Accept`,
// `Cratestack-Contract`), the compact response being turned back into named
// values, the request body going compact only once the op has answered
// compact, an unknown route and an error body staying named, and an adapter
// without hooks sending exactly what it always did. Run by `just verify-dart`
// against the generated `ci_rest` package (library name
// `dart_verify_ci_rest`).

import 'dart:convert';
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';
import 'package:flutter_test/flutter_test.dart';

const _compactCbor = 'application/vnd.cratestack.compact+cbor';
const _plainAccept = 'application/cbor, application/json;q=0.9';

/// Records every request and answers with the next of [answers].
class _Script implements CratestackHttpTransport {
  _Script(this.answers);

  final List<CratestackHttpResponse Function(CratestackHttpRequest)> answers;
  final List<CratestackHttpRequest> requests = [];

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    requests.add(request);
    return answers[requests.length - 1](request);
  }
}

Future<Uint8List> _named(CratestackCborCodec codec, Object? value) async =>
    codec.encodeJson(jsonEncode(value));

CratestackHttpResponse _cbor(int status, String type, Uint8List body) =>
    CratestackHttpResponse.buffered(
      status: status,
      headers: {'Content-Type': type},
      body: body,
    );

CratestackHttpAdapter _adapter(
  CratestackHttpTransport transport,
  CratestackCompactHooks? hooks,
) => CratestackHttpAdapter(
  transport,
  baseUri: Uri.parse('http://127.0.0.1:9'),
  compact: hooks,
);

const _author = {'id': 1, 'name': 'Ada'};

/// A reversible stand-in for `cratestack_cbor`'s `CompactKeys`, with the same
/// three tear-offs the hooks take. This fixture runs against the PUBLISHED
/// `cratestack_cbor` (`just verify-dart` resolves it from pub.dev), which
/// predates `createCompactKeys`; the real transcoding is proven by that
/// package's own suite, built from source. What this file proves is the
/// generated adapter seam: when each hook is called, with which op and
/// direction, and what goes on the wire.
class _StandInKeys {
  // A valid CBOR tag wrapping the named item, so structural validation applies.
  static const _marker = [0xD8, 0x2A];

  Uint8List toCompact(Uint8List named, String op, String direction) =>
      Uint8List.fromList([..._marker, ...named]);

  Uint8List fromCompact(Uint8List compact, String op, String direction) {
    if (compact.length < 2 ||
        compact[0] != _marker[0] ||
        compact[1] != _marker[1]) {
      throw StateError('not a stand-in compact body for $op ($direction)');
    }
    return Uint8List.sublistView(compact, 2);
  }

  /// Like the real binding: a selector only for an op the generated tables
  /// hold, `null` for any other, which is how the adapter knows to stay named.
  String? selector(String op) =>
      cratestackOpContracts.containsKey(op) ? 'stand-in:$op' : null;
}

void main() {
  late CratestackCborCodec codec;
  late _StandInKeys keys;
  late CratestackCompactHooks hooks;

  setUpAll(() async {
    codec = await createCborCodec();
    // The generated constant exists and is non-empty; the stand-in does
    // not read it.
    expect(cratestackKeyTables, isNotEmpty);
    keys = _StandInKeys();
    hooks = CratestackCompactHooks(
      toCompact: keys.toCompact,
      fromCompact: keys.fromCompact,
      selector: keys.selector,
    );
  });

  test('prepared compact bytes survive adapter recreation and validate contracts', () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      (_) => _cbor(200, _compactCbor, keys.toCompact(named, 'POST /authors', 'response')),
      (_) => _cbor(200, 'application/cbor', named),
      (_) => _cbor(200, 'application/cbor', named),
    ]);
    final adapter = CratestackHttpAdapter(transport, baseUri: Uri.parse('http://127.0.0.1:9'), compact: hooks, useRustTransport: true);
    const request = CratestackRequest(method: 'POST', path: '/api/authors', body: _author);
    await expectLater(adapter.prepare(const CratestackRequest(method: 'GET', path: '/api/authors'), options: const CratestackCallOptions(idempotencyKey: 'x')), throwsFormatException);
    await expectLater(adapter.prepare(const CratestackRequest(method: 'POST', path: '/api/other/authors', body: _author), options: const CratestackCallOptions(idempotencyKey: 'x')), throwsFormatException);
    await adapter.execute(request);
    await expectLater(adapter.prepare(request, options: const CratestackCallOptions(
      idempotencyKey: 'a', headers: {'idempotency-key': 'b'})), throwsFormatException);
    final saved = await adapter.prepare(request, options: const CratestackCallOptions(
      idempotencyKey: 'persisted', headers: {'Authorization': 'Bearer secret', 'X-API-Key': 'secret'}));
    expect(saved.toJson().toString(), isNot(contains('secret')));
    expect(transport.requests, hasLength(1));
    final unsafe = CratestackHttpAdapter(transport, baseUri: Uri.parse('http://user:secret@127.0.0.1:9'), compact: hooks);
    await expectLater(unsafe.prepare(request, options: const CratestackCallOptions(idempotencyKey: 'k')), throwsFormatException);
    final credentialUrl = saved.toJson()..['uri'] = 'http://user:secret@127.0.0.1:9/api/authors';
    await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(credentialUrl)), throwsFormatException);
    expect(transport.requests, hasLength(1));
    await adapter.executePrepared(saved);
    final restored = CratestackPreparedCall.fromJson(saved.toJson());
    await _adapter(transport, hooks).executePrepared(restored);
    expect(transport.requests[1].body, transport.requests[2].body);
    expect(transport.requests[1].headers['Content-Type'], _compactCbor);
    expect(transport.requests[2].headers['Idempotency-Key'], 'persisted');
    expect(transport.requests[1].extra[cratestackUseRustTransportExtraKey], isTrue);
    final moved = saved.toJson()..['uri'] = 'http://127.0.0.1:9/api/other/authors';
    await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(moved)), throwsFormatException);
    for (final invalid in [<int>[255], <int>[0, 0]]) {
      final malformed = saved.toJson()..['body'] = base64Encode(invalid);
      await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(malformed)), throwsFormatException);
    }
    final bad = saved.toJson()..['contract'] = 'old';
    await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(bad)), throwsFormatException);
    for (final entry in <String, Object?>{
      'keyTables': 'different', 'selector': 'wrong', 'version': 2,
      'uri': 'https://elsewhere.invalid/other',
    }.entries) {
      final altered = saved.toJson()..[entry.key] = entry.value;
      await expectLater(() async => adapter.executePrepared(CratestackPreparedCall.fromJson(altered)), throwsFormatException);
    }
    final injected = saved.toJson();
    (injected['headers'] as Map<String, dynamic>)['Authorization'] = 'Bearer injected';
    await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(injected)), throwsFormatException);
    expect(transport.requests, hasLength(3));
  });

  test('validates one CBOR item without changing the wire bytes', () async {
    final named = await _named(codec, _author);
    final transport = _Script(List.generate(13, (_) => (_) => _cbor(200, 'application/cbor', named)));
    final adapter = _adapter(transport, null);
    final saved = await adapter.prepare(const CratestackRequest(method: 'POST', path: '/api/authors', body: _author), options: const CratestackCallOptions(idempotencyKey: 'k'));
    List<int> hex(String value) => [for (var i = 0; i < value.length; i += 2) int.parse(value.substring(i, i + 2), radix: 16)];
    final invalid = ['', '0000', '18', '1c', 'ff', '81ff', 'bf00ff', '5f6161ff', '5f5fffff',
      'f800', 'f818', 'f81f', '61ff', '62c080', '63eda080', '64f4908080', '7f61c261a2ff'];
    final valid = ['a10102', 'a1810102', '1bffffffffffffffff', '3bffffffffffffffff', 'dbffffffffffffffff00',
      '9f8101bf0001ffff', '5f4100420102ff', '7f62c2a26161ff', 'f97e00', 'fa7f800000', 'f820', '1800'];
    invalid.add('${List.filled(257, '81').join()}00');
    valid.add('${List.filled(256, '81').join()}7f6161ff');
    for (final value in invalid) {
      final altered = saved.toJson()..['body'] = base64Encode(hex(value));
      await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(altered)), throwsFormatException);
    }
    expect(transport.requests, isEmpty);
    for (final value in valid) {
      final bytes = hex(value);
      final altered = saved.toJson()..['body'] = base64Encode(bytes);
      await adapter.executePrepared(CratestackPreparedCall.fromJson(altered));
      expect(transport.requests.last.body, bytes);
    }
  });

  test('full pin memory keeps old keys and leaves new keys named', () async {
    final named = await _named(codec, _author);
    final transport = _Script(List.generate(515, (_) => (_) =>
      _cbor(200, _compactCbor, keys.toCompact(named, 'POST /authors', 'response'))));
    final adapter = _adapter(transport, hooks);
    const request = CratestackRequest(method: 'POST', path: '/api/authors', body: _author);
    for (var i = 0; i < 512; i++) {
      final key = 'key-$i';
      await adapter.execute(request, options: CratestackCallOptions(idempotencyKey: key));
    }
    for (final key in ['overflow', 'key-0', 'overflow']) {
      await adapter.execute(request, options: CratestackCallOptions(idempotencyKey: key));
    }
    expect(transport.requests[512].headers['Content-Type'], 'application/cbor');
    expect(transport.requests[513].body, transport.requests[0].body);
    expect(transport.requests[514].body, transport.requests[512].body);
  });

  test('without hooks the wire is the one it always was', () async {
    final transport = _Script([
      (_) => _cbor(200, 'application/cbor', Uint8List(0)),
    ]);
    await _adapter(transport, null).execute(
      const CratestackRequest(method: 'GET', path: '/api/authors/1'),
    );
    final headers = transport.requests.single.headers;
    expect(headers['Accept'], _plainAccept);
    expect(headers.containsKey('Cratestack-Contract'), isFalse);
  });

  test('advertises compact, names the contract, and decodes a compact answer',
      () async {
    final compactBody = keys.toCompact(
      await _named(codec, _author),
      'GET /authors/{id}',
      'response',
    );
    final transport = _Script([(_) => _cbor(200, _compactCbor, compactBody)]);
    final value = await _adapter(transport, hooks).execute(
      const CratestackRequest(method: 'GET', path: '/api/authors/1'),
    );

    final request = transport.requests.single;
    expect(request.headers['Accept'], '$_compactCbor, $_plainAccept');
    expect(
      request.headers['Cratestack-Contract'],
      keys.selector('GET /authors/{id}'),
    );
    expect(request.headers.containsKey('Content-Type'), isFalse);
    expect(value, _author);
  });

  test('the request body goes compact only once the op answered compact',
      () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      // 1st: answered named, so the op is not known to be compact yet.
      (_) => _cbor(200, 'application/cbor', named),
      // 2nd: answers compact.
      (_) => _cbor(
        200,
        _compactCbor,
        keys.toCompact(named, 'POST /authors', 'response'),
      ),
      // 3rd: the op has now answered compact.
      (_) => _cbor(200, 'application/cbor', named),
    ]);
    final adapter = _adapter(transport, hooks);
    const request = CratestackRequest(
      method: 'POST',
      path: '/api/authors',
      body: _author,
    );

    await adapter.execute(request);
    expect(transport.requests[0].headers['Content-Type'], 'application/cbor');
    expect(transport.requests[0].body, named);

    expect(await adapter.execute(request), _author);
    expect(transport.requests[1].headers['Content-Type'], 'application/cbor');

    await adapter.execute(request);
    final third = transport.requests[2];
    expect(third.headers['Content-Type'], _compactCbor);
    expect(third.body, keys.toCompact(named, 'POST /authors', 'request'));
  });

  test('a retry under one Idempotency-Key reuses its first encoding',
      () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      (_) => _cbor(
        200,
        _compactCbor,
        keys.toCompact(named, 'POST /authors', 'response'),
      ),
      (_) => _cbor(200, 'application/cbor', named),
      (_) => _cbor(200, 'application/cbor', named),
    ]);
    final adapter = _adapter(transport, hooks);
    const request = CratestackRequest(
      method: 'POST',
      path: '/api/authors',
      body: _author,
    );
    const first = CratestackCallOptions(idempotencyKey: 'k-1');

    // The first attempt goes out named; the op then learns it is compact.
    await adapter.execute(request, options: first);
    // The retry keeps the named encoding, though the op is compact by now.
    await adapter.execute(request, options: first);
    expect(transport.requests[1].headers['Content-Type'], 'application/cbor');
    expect(transport.requests[1].body, transport.requests[0].body);

    // A fresh key picks up the compact encoding.
    await adapter.execute(
      request,
      options: const CratestackCallOptions(idempotencyKey: 'k-2'),
    );
    expect(transport.requests[2].headers['Content-Type'], _compactCbor);
  });

  test('a route the tables do not know stays named', () async {
    final transport = _Script([
      (_) => _cbor(200, 'application/cbor', Uint8List(0)),
    ]);
    await _adapter(transport, hooks).execute(
      const CratestackRequest(method: 'GET', path: '/api/nope/1'),
    );
    final headers = transport.requests.single.headers;
    expect(headers['Accept'], _plainAccept);
    expect(headers.containsKey('Cratestack-Contract'), isFalse);
  });

  test('the query string does not stop the op being named',
      () async {
    final transport = _Script([
      (_) => _cbor(200, 'application/cbor', Uint8List(0)),
    ]);
    await _adapter(transport, hooks).execute(
      const CratestackRequest(method: 'GET', path: '/api/posts/1?x=1'),
    );
    expect(
      transport.requests.single.headers['Cratestack-Contract'],
      keys.selector('GET /posts/{id}'),
    );
  });

  test('an error body stays named', () async {
    final error = await _named(codec, {
      'code': 'NOT_FOUND',
      'message': 'no such author',
    });
    final transport = _Script([(_) => _cbor(404, 'application/cbor', error)]);
    await expectLater(
      _adapter(transport, hooks).execute(
        const CratestackRequest(method: 'GET', path: '/api/authors/9'),
      ),
      throwsA(
        isA<CratestackRestException>()
            .having((e) => e.status, 'status', 404)
            .having((e) => e.code, 'code', 'NOT_FOUND'),
      ),
    );
  });

  test('a JSON adapter never goes compact', () async {
    final transport = _Script([
      (_) => CratestackHttpResponse.buffered(
        status: 200,
        headers: {'Content-Type': 'application/json'},
        body: utf8.encode(jsonEncode(_author)),
      ),
    ]);
    final adapter = CratestackHttpAdapter(
      transport,
      baseUri: Uri.parse('http://127.0.0.1:9'),
      format: CratestackWireFormat.json,
      compact: hooks,
    );
    expect(
      await adapter.execute(
        const CratestackRequest(method: 'GET', path: '/api/authors/1'),
      ),
      _author,
    );
    final headers = transport.requests.single.headers;
    expect(headers['Accept'], 'application/json');
    expect(headers.containsKey('Cratestack-Contract'), isFalse);
  });
}
