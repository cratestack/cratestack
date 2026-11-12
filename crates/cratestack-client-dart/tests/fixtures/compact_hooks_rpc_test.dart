// Compact wire keys over RPC: the adapter's `compact:` hooks, driven through
// a scripted transport. Unary calls use the real `createCompactKeys` from the
// published `cratestack_cbor`, built over the generated `cratestackKeyTables`;
// the stream case uses counting stand-in hooks so it can tell a transcoded
// item from a passed-through one. Covers
// the negotiation (`Accept`, `Cratestack-Contract`), a compact answer turned
// back into named values, the request body going compact only once the op has
// answered compact, a stream's compact items (and its never-compact error
// item), `/rpc/batch` and error bodies staying named, and an adapter without
// hooks sending what it always did. Run by `just verify-dart` against the
// generated `ci_rpc` package (library name `dart_verify_ci_rpc`).

import 'dart:convert';
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

const _compactCbor = 'application/vnd.cratestack.compact+cbor';
const _compactSeq = 'application/vnd.cratestack.compact+cbor-seq';
const _plainUnary = 'application/cbor, application/json;q=0.9';
const _plainStream =
    'application/cbor-seq, application/cbor, application/json;q=0.9';

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

CratestackHttpResponse _answer(int status, String type, List<int> body) =>
    CratestackHttpResponse.buffered(
      status: status,
      headers: {'Content-Type': type},
      body: body,
    );

CratestackRpcHttpAdapter _adapter(
  CratestackHttpTransport transport,
  CratestackCompactHooks? hooks,
) => CratestackRpcHttpAdapter(
  transport,
  baseUri: Uri.parse('http://127.0.0.1:9'),
  compact: hooks,
);

const _author = {'id': 1, 'name': 'Ada'};
const _get = 'model.Author.get';
const _create = 'model.Author.create';

void main() {
  late CratestackCborCodec codec;
  late CompactKeys keys;
  late CratestackCompactHooks hooks;

  setUpAll(() async {
    codec = await createCborCodec();
    // The real binding, built from the generated tables, as an app does.
    keys = await createCompactKeys(cratestackKeyTables);
    hooks = CratestackCompactHooks(
      toCompact: keys.toCompact,
      fromCompact: keys.fromCompact,
      selector: keys.selector,
    );
  });

  test('call option extra is not kept by a prepared call', () async {
    final transport = _Script([
      (_) => _answer(204, 'application/cbor', const <int>[]),
    ]);
    final adapter = _adapter(transport, null);
    final saved = await adapter.prepareCall(_create, _author, options: const CratestackRpcCallOptions(
      idempotencyKey: 'k', extra: {'k': 1}));
    expect(saved.toJson().toString(), isNot(contains("'k': 1")));
    await adapter.executePrepared(saved);
    expect(transport.requests.single.extra, isEmpty);
  });

  test('prepared compact bytes survive adapter recreation and validate contracts', () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      (_) => _answer(200, _compactCbor, keys.toCompact(named, _create, 'response')),
      (_) => _answer(200, 'application/cbor', named),
      (_) => _answer(200, 'application/cbor', named),
    ]);
    final adapter = _adapter(transport, hooks);
    await adapter.call(_create, _author);
    await expectLater(adapter.prepareCall(_create, _author, options: const CratestackRpcCallOptions(
      idempotencyKey: 'a', headers: {'idempotency-key': 'b'})), throwsFormatException);
    final saved = await adapter.prepareCall(_create, _author, options: const CratestackRpcCallOptions(
      idempotencyKey: 'persisted', headers: {'Authorization': 'Bearer secret', 'X-API-Key': 'secret'}));
    expect(saved.toJson().toString(), isNot(contains('secret')));
    expect(transport.requests, hasLength(1));
    final unsafe = CratestackRpcHttpAdapter(transport, baseUri: Uri.parse('http://user:secret@127.0.0.1:9'), compact: hooks);
    await expectLater(unsafe.prepareCall(_create, _author, options: const CratestackRpcCallOptions(idempotencyKey: 'k')), throwsFormatException);
    final credentialUrl = saved.toJson()..['uri'] = 'http://user:secret@127.0.0.1:9/rpc/$_create';
    await expectLater(adapter.executePrepared(CratestackPreparedCall.fromJson(credentialUrl)), throwsFormatException);
    expect(transport.requests, hasLength(1));
    await adapter.executePrepared(saved);
    final restored = CratestackPreparedCall.fromJson(saved.toJson());
    await _adapter(transport, hooks).executePrepared(restored);
    expect(transport.requests[1].body, transport.requests[2].body);
    expect(transport.requests[1].headers['Content-Type'], _compactCbor);
    expect(transport.requests[2].headers['Idempotency-Key'], 'persisted');
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
    final transport = _Script(List.generate(13, (_) => (_) => _answer(200, 'application/cbor', named)));
    final adapter = _adapter(transport, null);
    final saved = await adapter.prepareCall(_create, _author, options: const CratestackRpcCallOptions(idempotencyKey: 'k'));
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

  test('streaming scanner waits for complete text and simple values', () async {
    final text = CratestackCborSeqScanner();
    expect(await text.feed(Uint8List.fromList([0x62, 0xc2])), isEmpty);
    expect(await text.feed(Uint8List.fromList([0xa2])), [[0x62, 0xc2, 0xa2]]);
    final invalid = CratestackCborSeqScanner();
    expect(await invalid.feed(Uint8List.fromList([0x62, 0xc0])), isEmpty);
    expect(() => invalid.feed(Uint8List.fromList([0x80])), throwsFormatException);
    final simple = CratestackCborSeqScanner();
    expect(await simple.feed(Uint8List.fromList([0xf8])), isEmpty);
    expect(await simple.feed(Uint8List.fromList([0x20])), [[0xf8, 0x20]]);
    final invalidSimple = CratestackCborSeqScanner();
    expect(await invalidSimple.feed(Uint8List.fromList([0xf8])), isEmpty);
    expect(() => invalidSimple.feed(Uint8List.fromList([0x00])), throwsFormatException);
  });

  test('a full pin table evicts the oldest key and keeps recent ones', () async {
    final named = await _named(codec, _author);
    final transport = _Script(List.generate(515, (_) => (_) =>
      _answer(200, _compactCbor, keys.toCompact(named, _create, 'response'))));
    final adapter = _adapter(transport, hooks);

    for (var i = 0; i < 512; i++) {
      final key = 'key-$i';
      await adapter.call(_create, _author, options: CratestackRpcCallOptions(idempotencyKey: key));
    }
    for (final key in ['fresh', 'key-511', 'key-0']) {
      await adapter.call(_create, _author, options: CratestackRpcCallOptions(idempotencyKey: key));
    }
    // key-0 went out named (no compact answer yet); later keys compact.
    expect(transport.requests[0].headers['Content-Type'], 'application/cbor');
    // A new key past the window still follows negotiation.
    expect(transport.requests[512].headers['Content-Type'], _compactCbor);
    // A recent key's retry keeps its first encoding.
    expect(transport.requests[513].body, transport.requests[511].body);
    // key-0 was the oldest, evicted by 'fresh': it now follows negotiation.
    expect(transport.requests[514].headers['Content-Type'], _compactCbor);
  });

  test('without hooks the wire is the one it always was', () async {
    final transport = _Script([
      (_) => _answer(200, 'application/cbor', Uint8List(0)),
    ]);
    await _adapter(transport, null).call(_get, {'id': 1});
    final headers = transport.requests.single.headers;
    expect(headers['Accept'], _plainUnary);
    expect(headers.containsKey('Cratestack-Contract'), isFalse);
  });

  test('advertises compact, names the contract, and decodes a compact answer',
      () async {
    final named = await _named(codec, _author);
    final compactBody = keys.toCompact(named, _get, 'response');
    // Real integer keys: shorter than the named body, and reversible.
    expect(compactBody.length, lessThan(named.length));
    expect(keys.fromCompact(compactBody, _get, 'response'), named);
    final transport = _Script([
      (_) => _answer(200, _compactCbor, compactBody),
    ]);
    final value = await _adapter(transport, hooks).call(_get, {'id': 1});

    final request = transport.requests.single;
    expect(request.headers['Accept'], '$_compactCbor, $_plainUnary');
    expect(request.headers['Cratestack-Contract'], keys.selector(_get));
    expect(keys.selector(_get), hasLength(11));
    expect(request.headers['Content-Type'], 'application/cbor');
    expect(value, _author);
  });

  test('the request body goes compact only once the op answered compact',
      () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      (_) => _answer(200, 'application/cbor', named),
      (_) => _answer(200, _compactCbor, keys.toCompact(named, _create, 'response')),
      (_) => _answer(200, 'application/cbor', named),
    ]);
    final adapter = _adapter(transport, hooks);

    await adapter.call(_create, _author);
    expect(transport.requests[0].headers['Content-Type'], 'application/cbor');
    expect(transport.requests[0].body, named);

    expect(await adapter.call(_create, _author), _author);
    expect(transport.requests[1].headers['Content-Type'], 'application/cbor');

    await adapter.call(_create, _author);
    final third = transport.requests[2];
    expect(third.headers['Content-Type'], _compactCbor);
    expect(third.body, keys.toCompact(named, _create, 'request'));
  });

  test('a retry under one Idempotency-Key reuses its first encoding',
      () async {
    final named = await _named(codec, _author);
    final transport = _Script([
      (_) => _answer(200, _compactCbor, keys.toCompact(named, _create, 'response')),
      (_) => _answer(200, 'application/cbor', named),
      (_) => _answer(200, 'application/cbor', named),
    ]);
    final adapter = _adapter(transport, hooks);
    const first = CratestackRpcCallOptions(idempotencyKey: 'k-1');

    await adapter.call(_create, _author, options: first);
    await adapter.call(_create, _author, options: first);
    expect(transport.requests[1].headers['Content-Type'], 'application/cbor');
    expect(transport.requests[1].body, transport.requests[0].body);

    await adapter.call(
      _create,
      _author,
      options: const CratestackRpcCallOptions(idempotencyKey: 'k-2'),
    );
    expect(transport.requests[2].headers['Content-Type'], _compactCbor);
  });

  test('an op the tables do not know stays named', () async {
    final transport = _Script([
      (_) => _answer(200, 'application/cbor', Uint8List(0)),
    ]);
    await _adapter(transport, hooks).call('model.Nope.get', {'id': 1});
    final headers = transport.requests.single.headers;
    expect(headers['Accept'], _plainUnary);
    expect(headers.containsKey('Cratestack-Contract'), isFalse);
  });

  test('an error body stays named', () async {
    final error = await _named(codec, {
      'code': 'NOT_FOUND',
      'message': 'no such author',
    });
    final transport = _Script([(_) => _answer(404, 'application/cbor', error)]);
    await expectLater(
      _adapter(transport, hooks).call(_get, {'id': 9}),
      throwsA(
        isA<CratestackRpcException>()
            .having((e) => e.status, 'status', 404)
            .having((e) => e.body.code, 'code', 'NOT_FOUND'),
      ),
    );
  });

  test('/rpc/batch stays named', () async {
    final frames = await _named(codec, [
      {'id': 1, 'output': _author},
    ]);
    final transport = _Script([(_) => _answer(200, 'application/cbor', frames)]);
    final result = await _adapter(transport, hooks).batch(const [
      RpcRequest(id: 1, op: _get, input: {'id': 1}),
    ]);
    final request = transport.requests.single;
    expect(request.uri.path, '/rpc/batch');
    expect(request.headers['Accept'], _plainUnary);
    expect(request.headers.containsKey('Cratestack-Contract'), isFalse);
    expect(result.single.output, _author);
  });

  test('a JSON adapter never goes compact', () async {
    final transport = _Script([
      (_) => _answer(200, 'application/json', utf8.encode(jsonEncode(_author))),
    ]);
    final adapter = CratestackRpcHttpAdapter(
      transport,
      baseUri: Uri.parse('http://127.0.0.1:9'),
      format: CratestackWireFormat.json,
      compact: hooks,
    );
    expect(await adapter.call(_get, {'id': 1}), _author);
    expect(transport.requests.single.headers.containsKey('Cratestack-Contract'),
        isFalse);
  });

  group('streams', () {
    final calls = <String>[];
    // Stand-ins that leave the bytes alone and record what was asked of them,
    // so a transcoded item is told apart from one passed through.
    final counting = CratestackCompactHooks(
      toCompact: (bytes, op, direction) {
        calls.add('to:$op:$direction');
        return bytes;
      },
      fromCompact: (bytes, op, direction) {
        calls.add('from:$op:$direction');
        return bytes;
      },
      selector: (op) => 'sel-$op',
    );
    setUp(calls.clear);

    Future<Uint8List> items(List<Object?> values) async {
      final builder = BytesBuilder();
      for (final value in values) {
        builder.add(await _named(codec, value));
      }
      return builder.takeBytes();
    }

    test('compact items are transcoded and the error item is not', () async {
      final body = BytesBuilder()
        ..add(await items([
          {'n': 1},
          {'n': 2},
        ]))
        // The in-band error sentinel (tag 48900) wraps a named error body.
        ..add([0xd9, 0xbf, 0x04])
        ..add(await _named(codec, {'code': 'INTERNAL', 'message': 'boom'}));
      final transport = _Script([
        (_) => _answer(200, _compactSeq, body.takeBytes()),
      ]);
      final seen = <Object?>[];
      await expectLater(
        _adapter(transport, counting)
            .stream('procedure.searchPosts', const {})
            .map((item) {
              seen.add(item);
              return item;
            })
            .drain<void>(),
        throwsA(isA<CratestackRpcException>()),
      );

      final request = transport.requests.single;
      expect(request.headers['Accept'], '$_compactSeq, $_plainStream');
      expect(request.headers['Cratestack-Contract'], 'sel-procedure.searchPosts');
      expect(seen, [
        {'n': 1},
        {'n': 2},
      ]);
      expect(calls, [
        'from:procedure.searchPosts:response',
        'from:procedure.searchPosts:response',
      ]);
    });

    test('a plain cbor-seq answer is left alone', () async {
      // Built before the script: its answers are synchronous closures.
      final body = await items([
        {'n': 1},
      ]);
      final transport = _Script([
        (_) => _answer(200, 'application/cbor-seq', body),
      ]);
      final seen = await _adapter(transport, counting)
          .stream('procedure.searchPosts', const {})
          .toList();
      expect(seen, [
        {'n': 1},
      ]);
      expect(calls, isEmpty);
    });
  });
}
