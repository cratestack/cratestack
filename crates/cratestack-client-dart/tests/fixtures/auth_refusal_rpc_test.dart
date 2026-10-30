// cratestack#1243: an `AuthProvider` refusal over RPC. `RpcErrorBody` used to
// drop every member but `code`, `message` and `details`; it now keeps them in
// `extra`, and `CratestackRpcException` carries the answer's headers and the
// `WWW-Authenticate` challenges. Run by `just verify-dart` against the
// generated `ci_rpc` package.

import 'dart:convert';
import 'dart:io';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

const _renew = 'Signature error="attestation_expired"';

const _body = <String, Object?>{
  'code': 'unauthenticated',
  'message': 'attestation expired',
  'details': {'device': 'd-1'},
  'error': 'attestation_expired',
  'retry': {'after': 3},
};

class _Answer implements CratestackHttpTransport {
  _Answer(this.response);

  final CratestackHttpResponse response;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async => response;
}

Future<CratestackRpcException> _refusal(
  CratestackHttpTransport transport,
  Uri base, {
  bool batch = false,
}) async {
  final adapter = CratestackRpcHttpAdapter(transport, baseUri: base);
  try {
    if (batch) {
      await adapter.batch([const RpcRequest(id: 1, op: 'x', input: null)]);
    } else {
      await adapter.call('x', {});
    }
  } on CratestackRpcException catch (error) {
    return error;
  }
  fail('expected a CratestackRpcException');
}

void _expectRefusal(CratestackRpcException e) {
  expect(e.status, 401);
  expect(e.code, CratestackRpcErrorCodes.unauthenticated);
  expect(e.message, 'attestation expired');
  expect(e.details, {'device': 'd-1'});
  expect(e.extra, {
    'error': 'attestation_expired',
    'retry': {'after': 3},
  });
  expect(e.body.extra, e.extra);
}

CratestackHttpResponse _answer(String contentType, List<int> bytes) =>
    CratestackHttpResponse.buffered(
      status: 401,
      headers: {'Content-Type': contentType, 'WWW-Authenticate': _renew},
      body: bytes,
    );

void main() {
  final base = Uri.parse('http://127.0.0.1:9');

  for (final (name, contentType, bytes) in [
    ('CBOR', 'application/cbor', cbor.cbor.encode(_body)),
    ('JSON', 'application/json', utf8.encode(jsonEncode(_body))),
  ]) {
    test('a $name unary refusal keeps its details, members and challenge', () async {
      final e = await _refusal(_Answer(_answer(contentType, bytes)), base);
      _expectRefusal(e);
      expect(e.challenges, [_renew]);
      expect(e.headers['www-authenticate'], _renew);
    });
  }

  test('a refused batch is one exception with the same members and challenge', () async {
    final e = await _refusal(
      _Answer(_answer('application/cbor', cbor.cbor.encode(_body))),
      base,
      batch: true,
    );
    _expectRefusal(e);
    expect(e.challenges, [_renew]);
  });

  test('a batch frame error keeps its members and has no challenge', () {
    final frame = RpcResponseFrame.fromWire({'id': 1, 'error': _body});
    expect(frame.error!.extra['error'], 'attestation_expired');
    final e = CratestackRpcException(status: 401, body: frame.error!);
    expect(e.challenges, isEmpty);
  });

  test('an error without a refusal has no extra members and no challenge', () async {
    final e = await _refusal(
      _Answer(CratestackHttpResponse.buffered(
        status: 401,
        headers: {'Content-Type': 'application/json'},
        body: utf8.encode('{"code":"unauthenticated","message":"no credential"}'),
      )),
      base,
    );
    expect(e.details, isNull);
    expect(e.extra, isEmpty);
    expect(e.challenges, isEmpty);
  });

  test('through CratestackDioTransport, the challenge reaches the exception', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      await request.drain<void>();
      request.response.statusCode = 401;
      request.response.headers
        ..set('content-type', 'application/cbor')
        ..add('www-authenticate', _renew);
      request.response.add(cbor.cbor.encode(_body));
      await request.response.close();
    });
    final e = await _refusal(
      CratestackDioTransport(Dio()),
      Uri.parse('http://127.0.0.1:${server.port}'),
    );
    _expectRefusal(e);
    expect(e.challenges, [_renew]);
  });
}
