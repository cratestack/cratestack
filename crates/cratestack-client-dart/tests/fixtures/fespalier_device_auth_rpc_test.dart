// cratestack#1303, `--preset fespalier` (RPC): the generated `DeviceAuthServer`
// over fespalier_transport's `FakeTransport`. The REST twin is
// `fespalier_device_auth_rest_test.dart`.
//
// Proves, against the real `fespalier_sign_keypair` interface:
//  * each leg sends the request the schema says (path, `{args: ...}` body) and
//    maps the answer into `EnrolmentChallenge`, `DeviceEnrolment`,
//    `RoleChallenge` and `Attestation`;
//  * the fields the interface has no name for pass through
//    (`EnrolmentStart.fields`, `data`, `AuthUser.claims`), and `userOf`
//    replaces the user;
//  * a 403 is an `AuthRejected` on the legs the interface says, a failure
//    `fieldErrors` recognises is thrown as what it returns, and anything
//    else is thrown as it is;
//  * `revoke` is signed by the device (the attestation and signature
//    headers), and no other leg carries them;
//  * a caller's abort trigger (a page leaving) cannot abort a leg;
//  * `unsignedPaths` lists the paths the unsigned legs go to.
//
// `just verify-dart-fespalier` copies this file into the package generated
// from `device_auth_rpc.cstack` (library name `dart_verify_device_auth_rpc`).

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_device_auth_rpc/dart_verify_device_auth_rpc.dart';
import 'package:fespalier_auth/fespalier_auth.dart';
import 'package:fespalier_keypair/fespalier_keypair.dart';
import 'package:fespalier_keypair/testing.dart';
import 'package:fespalier_sign_keypair/fespalier_sign_keypair.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:fespalier_transport/testing.dart';
import 'package:flutter_test/flutter_test.dart';

final _base = Uri.parse('http://127.0.0.1:9');

DartVerifyDeviceAuthRpcDeviceAuthServer _server(
  HttpTransport transport, {
  Object? Function(Object error)? fieldErrors,
  AuthUser Function(EnrolledDevice answer)? userOf,
}) => DartVerifyDeviceAuthRpcDeviceAuthServer(
  transport,
  baseUri: _base,
  fieldErrors: fieldErrors,
  userOf: userOf,
);

Map<String, String> _jwk(String name) => {
  'kty': 'EC',
  'crv': 'P-256',
  'x': '$name-x',
  'y': '$name-y',
};

TransportResponse _answer(int status, Object? value) =>
    TransportResponse.fromBytes(
      statusCode: status,
      headers: const {'content-type': 'application/cbor'},
      bytes: Uint8List.fromList(cbor.cbor.encode(value)),
    );

/// An attestation `Attestation.parse` reads: three parts, claims with `exp`
/// and `cnf.jkt`. Nothing here is verified by the client.
String _jws(String jkt) {
  String part(Object value) =>
      base64Url.encode(utf8.encode(jsonEncode(value))).replaceAll('=', '');
  return '${part({'alg': 'ES256'})}.'
      '${part({'exp': 4102444800, 'cnf': {'jkt': jkt}})}.c2ln';
}

Object? _body(SentRequest sent) => cbor.cbor.decode(sent.body);

void main() {
  group('enrolment', () {
    test('beginEnrolment sends the keys and the app fields, maps the challenges', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, {
          'enrolmentId': 'e-1',
          'ambient': 'challenge-a',
          'userPresent': 'challenge-u',
        }),
      );
      final challenge = await _server(fake).beginEnrolment(
        EnrolmentStart(
          keys: {
            KeyRole.ambient: _jwk('a'),
            KeyRole.userPresent: _jwk('u'),
          },
          fields: const {'phone': '+237600000000', 'otp': '424242'},
        ),
      );
      final sent = fake.requests.single;
      expect(sent.method, 'POST');
      expect(sent.url.path, '/rpc/procedure.beginEnrolment');
      expect(_body(sent), {
        'args': {
          'phone': '+237600000000',
          'otp': '424242',
          'ambient': _jwk('a'),
          'userPresent': _jwk('u'),
        },
      });
      expect(challenge.challenges, {
        KeyRole.ambient: 'challenge-a',
        KeyRole.userPresent: 'challenge-u',
      });
      expect(challenge.data, {'enrolmentId': 'e-1'});
    });

    test('a device with no user-present key sends none and gets one challenge', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, {'enrolmentId': 'e-2', 'ambient': 'challenge-a'}),
      );
      final challenge = await _server(fake).beginEnrolment(
        EnrolmentStart(keys: {KeyRole.ambient: _jwk('a')}),
      );
      final args = (_body(fake.requests.single)! as Map)['args'] as Map;
      expect(args['userPresent'], isNull);
      expect(args['phone'], '', reason: 'a field the app did not send is empty text');
      expect(challenge.challenges, {KeyRole.ambient: 'challenge-a'});
    });

    test('confirmEnrolment sends the signatures and what leg one answered, maps the device', () async {
      final fake = FakeTransport.script([
        _answer(200, {'enrolmentId': 'e-1', 'ambient': 'ca', 'userPresent': 'cu'}),
        _answer(200, {
          'deviceId': 'dev-1',
          'userId': 'user-9',
          'phone': '+237600000000',
          'ambient': _jws('jkt-a'),
          'userPresent': _jws('jkt-u'),
        }),
      ]);
      final server = _server(fake);
      final challenge = await server.beginEnrolment(
        EnrolmentStart(keys: {
          KeyRole.ambient: _jwk('a'),
          KeyRole.userPresent: _jwk('u'),
        }),
      );
      final enrolment = await server.confirmEnrolment(
        challenge,
        const {KeyRole.ambient: 'sig-a', KeyRole.userPresent: 'sig-u'},
        'ignored-proof',
      );
      final sent = fake.requests.last;
      expect(sent.url.path, '/rpc/procedure.confirmEnrolment');
      expect(_body(sent), {
        'args': {
          'enrolmentId': 'e-1',
          'ambient': 'sig-a',
          'userPresent': 'sig-u',
        },
      });
      expect(enrolment.deviceId, 'dev-1');
      expect(enrolment.attestations.keys.toSet(), {KeyRole.ambient, KeyRole.userPresent});
      expect(enrolment.attestations[KeyRole.ambient]!.jkt, 'jkt-a');
      expect(enrolment.attestations[KeyRole.userPresent]!.jkt, 'jkt-u');
      // The convention: userId is the id, the rest of the answer is claims.
      expect(enrolment.user.id, 'user-9');
      expect(enrolment.user.claims, {'phone': '+237600000000'});
    });

    test('userOf builds the signed-in user itself', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, {
          'deviceId': 'dev-1',
          'userId': 'user-9',
          'phone': '+237600000000',
          'ambient': _jws('jkt-a'),
        }),
      );
      final enrolment = await _server(
        fake,
        userOf: (answer) => AuthUser(id: answer.userId, name: answer.phone),
      ).confirmEnrolment(
        const EnrolmentChallenge(
          challenges: {KeyRole.ambient: 'c'},
          data: {'enrolmentId': 'e-1'},
        ),
        const {KeyRole.ambient: 'sig-a'},
        null,
      );
      expect(enrolment.user.name, '+237600000000');
      expect(enrolment.user.claims, isEmpty);
      expect(enrolment.attestations.keys.toSet(), {KeyRole.ambient});
    });

    test('sendEnrolmentCode is a call of its own that answers what the procedure answers', () async {
      final fake = FakeTransport(
        (_, __) => _answer(200, {'phone': '+237600000000'}),
      );
      final sent = await _server(fake).sendEnrolmentCode(
        const SendEnrolmentCodeArgs(args: SendCodeArgs(phone: '+237600000000')),
      );
      expect(fake.requests.single.url.path, '/rpc/procedure.sendEnrolmentCode');
      expect(_body(fake.requests.single), {
        'args': {'phone': '+237600000000'},
      });
      expect(sent.phone, '+237600000000');
    });
  });

  group('renewal', () {
    test('beginRenewal sends the device and the role, confirmRenewal the signature', () async {
      final fake = FakeTransport.script([
        _answer(200, {'challengeId': 'c-1', 'challenge': 'text-to-sign'}),
        _answer(200, {'attestation': _jws('jkt-new')}),
      ]);
      final server = _server(fake);
      final challenge = await server.beginRenewal('dev-1', KeyRole.userPresent);
      expect(fake.requests.single.url.path, '/rpc/procedure.beginRenewal');
      expect(_body(fake.requests.single), {
        'args': {'deviceId': 'dev-1', 'role': 'userPresent'},
      });
      expect(challenge.role, KeyRole.userPresent);
      expect(challenge.challenge, 'text-to-sign');
      expect(challenge.data, {'challengeId': 'c-1'});

      final attestation = await server.confirmRenewal(challenge, 'the-signature');
      expect(fake.requests.last.url.path, '/rpc/procedure.confirmRenewal');
      expect(_body(fake.requests.last), {
        'args': {'challengeId': 'c-1', 'signature': 'the-signature'},
      });
      expect(attestation.jkt, 'jkt-new');
    });
  });

  group('failures', () {
    final forbidden = {'code': 'forbidden', 'message': 'no'};

    test('a 403 is an AuthRejected on both renewal legs and on confirmEnrolment', () async {
      Future<Object> thrown(Future<Object?> Function(DartVerifyDeviceAuthRpcDeviceAuthServer) call) async {
        final server = _server(FakeTransport((_, __) => _answer(403, forbidden)));
        try {
          await call(server);
        } catch (error) {
          return error;
        }
        fail('expected the call to throw');
      }

      expect(await thrown((s) => s.beginRenewal('dev-1', KeyRole.ambient)), isA<AuthRejected>());
      expect(
        await thrown(
          (s) => s.confirmRenewal(
            const RoleChallenge(role: KeyRole.ambient, challenge: 'c', data: {'challengeId': 'c-1'}),
            'sig',
          ),
        ),
        isA<AuthRejected>(),
      );
      expect(
        await thrown(
          (s) => s.confirmEnrolment(
            const EnrolmentChallenge(challenges: {KeyRole.ambient: 'c'}, data: {'enrolmentId': 'e'}),
            const {KeyRole.ambient: 'sig'},
            null,
          ),
        ),
        isA<AuthRejected>(),
      );
      // The server saying no to leg one of an enrolment is not "the device is dead".
      expect(
        await thrown(
          (s) => s.beginEnrolment(EnrolmentStart(keys: {KeyRole.ambient: _jwk('a')})),
        ),
        isA<CratestackRpcException>().having((e) => e.status, 'status', 403),
      );
    });

    test('a 5xx and being offline are thrown as they are: the session stays', () async {
      await expectLater(
        _server(FakeTransport((_, __) => _answer(503, {'code': 'unavailable', 'message': 'x'})))
            .beginRenewal('dev-1', KeyRole.ambient),
        throwsA(isA<CratestackRpcException>().having((e) => e.status, 'status', 503)),
      );
      await expectLater(
        _server(FakeTransport((_, __) => throw const TransportUnsent('offline')))
            .beginRenewal('dev-1', KeyRole.ambient),
        throwsA(isA<CratestackTransportUnsent>()),
      );
    });

    test('fieldErrors turns a 422 into what a form shows, and null leaves it', () async {
      final unprocessable = {'code': 'invalid_argument', 'message': 'wrong code'};
      final mapped = _Problem('otp');
      await expectLater(
        _server(
          FakeTransport((_, __) => _answer(422, unprocessable)),
          fieldErrors: (error) =>
              error is CratestackRpcException && error.status == 422 ? mapped : null,
        ).beginEnrolment(EnrolmentStart(keys: {KeyRole.ambient: _jwk('a')})),
        throwsA(same(mapped)),
      );
      await expectLater(
        _server(
          FakeTransport((_, __) => _answer(422, unprocessable)),
          fieldErrors: (error) => null,
        ).beginEnrolment(EnrolmentStart(keys: {KeyRole.ambient: _jwk('a')})),
        throwsA(isA<CratestackRpcException>().having((e) => e.status, 'status', 422)),
      );
      await expectLater(
        _server(FakeTransport((_, __) => _answer(422, unprocessable)))
            .beginEnrolment(EnrolmentStart(keys: {KeyRole.ambient: _jwk('a')})),
        throwsA(isA<CratestackRpcException>()),
      );
    });
  });

  group('revoke', () {
    final credential = DeviceCredential(
      deviceId: 'dev-1',
      attestations: {KeyRole.ambient: Attestation.parse(_jws('jkt-a'))},
    );
    final signer = DeviceSigner(
      keys: DeviceKeys(ambient: FakeKeypairSigner(keyId: 'ambient')),
      profile: DeviceSignatureProfile(audience: _base.toString()),
    );

    test('is signed by the device with its attestation, and no other leg is', () async {
      final fake = FakeTransport.script([
        _answer(200, {'deviceId': 'dev-1'}),
        _answer(200, {'challengeId': 'c-1', 'challenge': 'text'}),
      ]);
      final server = _server(fake);
      await server.revoke(credential, signer);
      final sent = fake.requests.single;
      expect(sent.url.path, '/rpc/procedure.revokeDevice');
      expect(_body(sent), {
        'args': {'deviceId': 'dev-1'},
      });
      expect(sent.lowerHeaders['x-attestation'], credential.attestation(KeyRole.ambient)!.jws);
      expect(sent.lowerHeaders['x-signature'], isNotEmpty);

      await server.beginRenewal('dev-1', KeyRole.ambient);
      expect(
        fake.requests.last.lowerHeaders.keys,
        isNot(anyOf(contains('x-attestation'), contains('x-signature'))),
      );
    });

    test('a server that refuses it throws, and is not an AuthRejected', () async {
      final fake = FakeTransport((_, __) => _answer(403, {'code': 'forbidden', 'message': 'no'}));
      await expectLater(
        _server(fake).revoke(credential, signer),
        throwsA(isA<CratestackRpcException>()),
      );
    });
  });

  group('abort isolation', () {
    test('a caller that leaves does not abort a leg other requests wait for', () async {
      final gate = Completer<TransportResponse>();
      final fake = FakeTransport((_, __) => gate.future);
      final leaves = Completer<void>();
      final call = TransportZone.run(
        leaves.future,
        () => _server(fake).beginRenewal('dev-1', KeyRole.ambient),
      );
      await pumpEventQueue();
      expect(fake.requests, hasLength(1));
      leaves.complete();
      await pumpEventQueue();
      gate.complete(_answer(200, {'challengeId': 'c-1', 'challenge': 'text'}));
      final challenge = await call;
      expect(challenge.challenge, 'text');
      expect(fake.abortCount, 0);
    });
  });

  test('unsignedPaths are the paths of the legs that go out with no session', () {
    expect(
      DartVerifyDeviceAuthRpcDeviceAuthServer.unsignedPaths(Uri.parse('https://api.example.com/v1/')),
      [
        '/v1/rpc/procedure.sendEnrolmentCode',
        '/v1/rpc/procedure.beginEnrolment',
        '/v1/rpc/procedure.confirmEnrolment',
        '/v1/rpc/procedure.beginRenewal',
        '/v1/rpc/procedure.confirmRenewal',
      ],
    );
    expect(
      DartVerifyDeviceAuthRpcDeviceAuthServer.unsignedPaths(Uri.parse('https://api.example.com')).first,
      '/rpc/procedure.sendEnrolmentCode',
    );
  });
}

/// What a form shows: stands in for `FieldErrors`.
final class _Problem {
  _Problem(this.field);

  final String field;
}
