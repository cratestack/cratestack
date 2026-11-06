// cratestack#1244: a step-up refusal over REST, read through
// `CratestackRestException.stepUp`, and `retryAfterStepUp`, which retries a
// call at most once after a re-proof. Run by `just verify-dart` against the
// generated `ci_rest` package.

import 'dart:convert';

import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, Object?> _envelope({Object? details, String error = 'insufficient_user_authentication'}) => {
      'code': 'UNAUTHORIZED',
      'message': 'step-up authentication required',
      'details': details ??
          {
            'max_age': 300,
            'acr_values': ['aal2', 'aal3'],
            'unmet': ['max_age', 'acr'],
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

Future<Object?> _get(CratestackHttpTransport transport) => CratestackHttpAdapter(
      transport,
      baseUri: Uri.parse('http://127.0.0.1:9'),
      format: CratestackWireFormat.json,
    ).execute(const CratestackRequest(method: 'GET', path: '/x'));

Future<CratestackRestException> _refusal(int status, Map<String, Object?> body) async {
  try {
    await _get(_Script([(status, body)]));
  } on CratestackRestException catch (error) {
    return error;
  }
  fail('expected a CratestackRestException');
}

void main() {
  test('a step-up refusal is typed from its details', () async {
    final stepUp = (await _refusal(401, _envelope())).stepUp!;
    expect(stepUp.maxAge, 300);
    expect(stepUp.acrValues, ['aal2', 'aal3']);
    expect(stepUp.amr, isEmpty);
    expect(stepUp.unmet, ['max_age', 'acr']);
  });

  test('a plain 401, another refusal or another status is not a step-up', () async {
    expect((await _refusal(401, {'code': 'UNAUTHORIZED', 'message': 'no'})).stepUp, isNull);
    expect((await _refusal(401, _envelope(error: 'invalid_token'))).stepUp, isNull);
    expect((await _refusal(403, _envelope())).stepUp, isNull);
  });

  test('mistyped details read as absent, never a throw', () async {
    for (final details in <Object>[
      {'max_age': '300', 'acr_values': 'aal2', 'amr': [1, 'pwd'], 'unmet': {}},
      'nope',
      [1, 2],
    ]) {
      final stepUp = (await _refusal(401, _envelope(details: details))).stepUp!;
      expect(stepUp.maxAge, isNull);
      expect(stepUp.acrValues, isEmpty);
      expect(stepUp.unmet, isEmpty);
    }
  });

  test('retryAfterStepUp retries once after a re-proof', () async {
    final transport = _Script([(401, _envelope()), (200, {'ok': true})]);
    StepUpRequired? asked;
    final result = await retryAfterStepUp(() => _get(transport), (stepUp) async {
      asked = stepUp;
      return true;
    });
    expect(result, {'ok': true});
    expect(asked!.maxAge, 300);
    expect(transport.sent, 2);
  });

  test('a second step-up is thrown, not retried', () async {
    final transport = _Script([(401, _envelope()), (401, _envelope()), (200, null)]);
    var reproofs = 0;
    await expectLater(
      retryAfterStepUp(() => _get(transport), (_) async {
        reproofs++;
        return true;
      }),
      throwsA(isA<CratestackRestException>().having((e) => e.stepUp, 'stepUp', isNotNull)),
    );
    expect((transport.sent, reproofs), (2, 1));
  });

  test('a declined re-proof rethrows; another error never asks for one', () async {
    final declined = _Script([(401, _envelope())]);
    await expectLater(
      retryAfterStepUp(() => _get(declined), (_) async => false),
      throwsA(isA<CratestackRestException>()),
    );
    expect(declined.sent, 1);

    final other = _Script([(500, {'code': 'INTERNAL', 'message': 'boom'})]);
    await expectLater(
      retryAfterStepUp(() => _get(other), (_) async => fail('no re-proof for a 500')),
      throwsA(isA<CratestackRestException>().having((e) => e.status, 'status', 500)),
    );
  });
}
