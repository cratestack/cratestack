// cratestack#1243: an `AuthProvider` refusal over REST, read through the typed
// getters of `CratestackRestException`: the fixed `code`, `message`,
// `details`, the extra members (never dropped, CBOR or JSON) and the
// `WWW-Authenticate` challenges. Run by `just verify-dart` against the
// generated `ci_rest` package.

import 'dart:convert';
import 'dart:io';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

const _renew = 'Signature error="attestation_expired"';

const _envelope = <String, Object?>{
  'code': 'UNAUTHORIZED',
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

Future<CratestackRestException> _refusal(CratestackHttpTransport transport, Uri base) async {
  try {
    await CratestackHttpAdapter(transport, baseUri: base).execute(
      const CratestackRequest(method: 'GET', path: '/x'),
    );
  } on CratestackRestException catch (error) {
    return error;
  }
  fail('expected a CratestackRestException');
}

void _expectRefusal(CratestackRestException e) {
  expect(e.status, 401);
  expect(e.code, 'UNAUTHORIZED');
  expect(e.message, 'attestation expired');
  expect(e.details, {'device': 'd-1'});
  expect(e.extra, {
    'error': 'attestation_expired',
    'retry': {'after': 3},
  });
  expect(e.body, _envelope, reason: 'the raw body keeps every member too');
}

void main() {
  final base = Uri.parse('http://127.0.0.1:9');

  for (final (name, contentType, bytes) in [
    ('CBOR', 'application/cbor', cbor.cbor.encode(_envelope)),
    ('JSON', 'application/json', utf8.encode(jsonEncode(_envelope))),
  ]) {
    test('a $name refusal keeps its code, details, members and challenge', () async {
      final e = await _refusal(
        _Answer(CratestackHttpResponse.buffered(
          status: 401,
          headers: {'Content-Type': contentType, 'WWW-Authenticate': _renew},
          body: bytes,
        )),
        base,
      );
      _expectRefusal(e);
      expect(e.challenges, [_renew]);
    });
  }

  test('an error without a refusal has no extra members and no challenge', () async {
    final e = await _refusal(
      _Answer(CratestackHttpResponse.buffered(
        status: 401,
        headers: {'Content-Type': 'application/json'},
        body: utf8.encode('{"code":"UNAUTHORIZED","message":"no credential"}'),
      )),
      base,
    );
    expect(e.code, 'UNAUTHORIZED');
    expect(e.details, isNull);
    expect(e.extra, isEmpty);
    expect(e.challenges, isEmpty);
  });

  test('a body that is not an envelope types as null, not a throw', () async {
    final e = await _refusal(
      _Answer(CratestackHttpResponse.buffered(
        status: 502,
        headers: {'Content-Type': 'text/plain'},
        body: utf8.encode('Bad Gateway'),
      )),
      base,
    );
    expect(e.code, isNull);
    expect(e.message, isNull);
    expect(e.extra, isEmpty);
  });

  test('through CratestackDioTransport, two challenge lines arrive joined', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      request.response.statusCode = 401;
      request.response.headers
        ..set('content-type', 'application/json')
        ..add('www-authenticate', _renew)
        ..add('www-authenticate', 'Bearer realm="api"');
      request.response.write(jsonEncode(_envelope));
      await request.response.close();
    });
    final e = await _refusal(
      CratestackDioTransport(Dio()),
      Uri.parse('http://127.0.0.1:${server.port}'),
    );
    _expectRefusal(e);
    expect(e.challenges, hasLength(1));
    expect(e.challenges.single, contains(_renew));
    expect(e.challenges.single, contains('Bearer realm="api"'));
  });
}
