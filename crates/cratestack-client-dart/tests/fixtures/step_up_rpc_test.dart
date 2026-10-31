// cratestack#1244: a step-up refusal over RPC, unary and in a batch frame,
// read through `CratestackRpcException.stepUp`, and `retryAfterStepUp`, which
// retries a call at most once after a re-proof. Run by `just verify-dart`
// against the generated `ci_rpc` package.

import 'dart:convert';

import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, Object?> _body({
  Object? details,
  String code = 'unauthenticated',
  String error = 'insufficient_user_authentication',
}) =>
    {
      'code': code,
      'message': 'step-up authentication required',
      'details': details ??
          {
            'amr': ['hwk'],
            'unmet': ['amr'],
          },
      'error': error,
    };

/// Answers each request with the next of [answers] (status and body).
class _Script implements CratestackHttpTransport {
  _Script(this.answers);

  final List<(int, Object?)> answers;
  int sent = 0;

  @override
  Future<CratestackHttpResponse> send(CratestackHttpRequest request) async {
    final (status, body) = answers[sent++];
    return CratestackHttpResponse.buffered(
      status: status,
      headers: {'Content-Type': 'application/json'},
      body: utf8.encode(jsonEncode(body)),
    );
  }
}

Future<Object?> _call(CratestackHttpTransport transport) => CratestackRpcHttpAdapter(
      transport,
      baseUri: Uri.parse('http://127.0.0.1:9'),
      format: CratestackWireFormat.json,
    ).call('x', {});

Future<CratestackRpcException> _refusal(Map<String, Object?> body) async {
  try {
    await _call(_Script([(401, body)]));
  } on CratestackRpcException catch (error) {
    return error;
  }
  fail('expected a CratestackRpcException');
}

void main() {
  test('a unary step-up refusal is typed from its details', () async {
    final stepUp = (await _refusal(_body())).stepUp!;
    expect(stepUp.maxAge, isNull);
    expect(stepUp.acrValues, isEmpty);
    expect(stepUp.amr, ['hwk']);
    expect(stepUp.unmet, ['amr']);
  });

  test('a batch frame reads the same', () {
    final frame = RpcResponseFrame.fromWire({
      'id': 1,
      'error': _body(details: {'max_age': 60, 'unmet': ['max_age']}),
    });
    final stepUp = CratestackRpcException(status: 401, body: frame.error!).stepUp!;
    expect(stepUp.maxAge, 60);
    expect(stepUp.unmet, ['max_age']);
  });

  test('a plain unauthenticated or another code is not a step-up', () async {
    expect((await _refusal({'code': 'unauthenticated', 'message': 'no'})).stepUp, isNull);
    expect((await _refusal(_body(error: 'attestation_expired'))).stepUp, isNull);
    expect((await _refusal(_body(code: 'permission_denied'))).stepUp, isNull);
  });

  test('mistyped details read as absent, never a throw', () async {
    for (final details in <Object>[
      {'max_age': 1.5, 'acr_values': {}, 'amr': 'hwk', 'unmet': [null]},
      'nope',
    ]) {
      final stepUp = (await _refusal(_body(details: details))).stepUp!;
      expect(stepUp.maxAge, isNull);
      expect(stepUp.amr, isEmpty);
      expect(stepUp.unmet, isEmpty);
    }
  });

  test('retryAfterStepUp retries once after a re-proof', () async {
    final transport = _Script([(401, _body()), (200, {'ok': true})]);
    final result = await retryAfterStepUp(() => _call(transport), (stepUp) async {
      expect(stepUp.amr, ['hwk']);
      return true;
    });
    expect(result, {'ok': true});
    expect(transport.sent, 2);
  });

  test('a second step-up is thrown, not retried', () async {
    final transport = _Script([(401, _body()), (401, _body()), (200, null)]);
    var reproofs = 0;
    await expectLater(
      retryAfterStepUp(() => _call(transport), (_) async {
        reproofs++;
        return true;
      }),
      throwsA(isA<CratestackRpcException>().having((e) => e.stepUp, 'stepUp', isNotNull)),
    );
    expect((transport.sent, reproofs), (2, 1));
  });

  test('a declined re-proof rethrows the first refusal', () async {
    final transport = _Script([(401, _body())]);
    await expectLater(
      retryAfterStepUp(() => _call(transport), (_) async => false),
      throwsA(isA<CratestackRpcException>()),
    );
    expect(transport.sent, 1);
  });
}
