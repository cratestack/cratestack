// cratestack#1303, `--preset fespalier` (REST): the generated `DeviceAuthServer`
// over fespalier_transport's `FakeTransport`. The RPC twin is
// `fespalier_device_auth_rpc_test.dart`, which proves the failure mapping, the
// signed `revoke` and the abort isolation in full; this file proves what is
// different on REST and in the `device_auth_rest.cstack` shape:
//  * the legs go to `<basePath>/$procs/<name>` and a 403 is the REST
//    exception, still an `AuthRejected`;
//  * the one-time code goes with leg two (`proof: "otp"`), and leg one sends
//    the phone number only;
//  * the answer's `name` and `email` are the user's, the rest are claims;
//  * `unsignedPaths` carries the base path.
//
// `just verify-dart-fespalier` copies this file into the package generated
// from `device_auth_rest.cstack` (library name `dart_verify_device_auth_rest`).

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:cbor/simple.dart' as cbor;
import 'package:dart_verify_device_auth_rest/dart_verify_device_auth_rest.dart';
import 'package:fespalier_auth/fespalier_auth.dart';
import 'package:fespalier_keypair/fespalier_keypair.dart';
import 'package:fespalier_keypair/testing.dart';
import 'package:fespalier_sign_keypair/fespalier_sign_keypair.dart';
import 'package:fespalier_transport/fespalier_transport.dart';
import 'package:fespalier_transport/testing.dart';
import 'package:flutter_test/flutter_test.dart';

final _base = Uri.parse('http://127.0.0.1:9');

DartVerifyDeviceAuthRestDeviceAuthServer _server(HttpTransport transport) =>
    DartVerifyDeviceAuthRestDeviceAuthServer(transport, baseUri: _base);

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

String _jws(String jkt) {
  String part(Object value) =>
      base64Url.encode(utf8.encode(jsonEncode(value))).replaceAll('=', '');
  return '${part({'alg': 'ES256'})}.'
      '${part({'exp': 4102444800, 'cnf': {'jkt': jkt}})}.c2ln';
}

Object? _body(SentRequest sent) => cbor.cbor.decode(sent.body);

void main() {
  test('leg one sends the keys and the phone number to the REST route', () async {
    final fake = FakeTransport(
      (_, __) => _answer(200, {'enrolmentId': 'e-1', 'ambient': 'ca', 'userPresent': 'cu'}),
    );
    final challenge = await _server(fake).beginEnrolment(
      EnrolmentStart(
        keys: {KeyRole.ambient: _jwk('a'), KeyRole.userPresent: _jwk('u')},
        fields: const {'phone': '+237600000000'},
      ),
    );
    final sent = fake.requests.single;
    expect(sent.method, 'POST');
    expect(sent.url.path, '/api/\$procs/beginEnrolment');
    expect(_body(sent), {
      'args': {
        'phone': '+237600000000',
        'ambient': _jwk('a'),
        'userPresent': _jwk('u'),
      },
    });
    expect(challenge.challenges, {KeyRole.ambient: 'ca', KeyRole.userPresent: 'cu'});
    expect(challenge.data, {'enrolmentId': 'e-1'});
  });

  test('leg two sends the proof, the signatures and the enrolment id, and names the user', () async {
    final fake = FakeTransport(
      (_, __) => _answer(200, {
        'deviceId': 'dev-1',
        'userId': 'user-9',
        'name': 'Ada',
        'email': 'ada@example.com',
        'phone': '+237600000000',
        'ambient': _jws('jkt-a'),
      }),
    );
    final enrolment = await _server(fake).confirmEnrolment(
      const EnrolmentChallenge(
        challenges: {KeyRole.ambient: 'ca'},
        data: {'enrolmentId': 'e-1'},
      ),
      const {KeyRole.ambient: 'sig-a'},
      '424242',
    );
    final sent = fake.requests.single;
    expect(sent.url.path, '/api/\$procs/confirmEnrolment');
    expect(_body(sent), {
      'args': {
        'enrolmentId': 'e-1',
        'otp': '424242',
        'ambient': 'sig-a',
        'userPresent': null,
      },
    });
    expect(enrolment.user.id, 'user-9');
    expect(enrolment.user.name, 'Ada');
    expect(enrolment.user.email, 'ada@example.com');
    expect(enrolment.user.claims, {'phone': '+237600000000'});
    expect(enrolment.attestations.keys.toSet(), {KeyRole.ambient});
  });

  test('a 403 on a renewal is an AuthRejected, a 5xx is the REST exception', () async {
    await expectLater(
      _server(FakeTransport((_, __) => _answer(403, {'code': 'forbidden', 'message': 'no'})))
          .beginRenewal('dev-1', KeyRole.ambient),
      throwsA(isA<AuthRejected>()),
    );
    await expectLater(
      _server(FakeTransport((_, __) => _answer(503, {'code': 'unavailable', 'message': 'x'})))
          .beginRenewal('dev-1', KeyRole.ambient),
      throwsA(isA<CratestackRestException>().having((e) => e.status, 'status', 503)),
    );
  });

  test('renewal sends the device and the role, then the signature', () async {
    final fake = FakeTransport.script([
      _answer(200, {'challengeId': 'c-1', 'challenge': 'text'}),
      _answer(200, {'attestation': _jws('jkt-new')}),
    ]);
    final server = _server(fake);
    final challenge = await server.beginRenewal('dev-1', KeyRole.ambient);
    final attestation = await server.confirmRenewal(challenge, 'sig');
    expect(fake.requests.first.url.path, '/api/\$procs/beginRenewal');
    expect(_body(fake.requests.first), {
      'args': {'deviceId': 'dev-1', 'role': 'ambient'},
    });
    expect(fake.requests.last.url.path, '/api/\$procs/confirmRenewal');
    expect(_body(fake.requests.last), {
      'args': {'challengeId': 'c-1', 'signature': 'sig'},
    });
    expect(attestation.jkt, 'jkt-new');
  });

  test('revoke is signed by the device', () async {
    final credential = DeviceCredential(
      deviceId: 'dev-1',
      attestations: {KeyRole.ambient: Attestation.parse(_jws('jkt-a'))},
    );
    final signer = DeviceSigner(
      keys: DeviceKeys(ambient: FakeKeypairSigner(keyId: 'ambient')),
      profile: DeviceSignatureProfile(audience: _base.toString()),
    );
    final fake = FakeTransport((_, __) => _answer(200, {'deviceId': 'dev-1'}));
    await _server(fake).revoke(credential, signer);
    final sent = fake.requests.single;
    expect(sent.url.path, '/api/\$procs/revokeDevice');
    expect(sent.lowerHeaders['x-attestation'], credential.attestation(KeyRole.ambient)!.jws);
    expect(sent.lowerHeaders['x-signature'], isNotEmpty);
  });

  test('a caller that leaves does not abort a leg', () async {
    final gate = Completer<TransportResponse>();
    final fake = FakeTransport((_, __) => gate.future);
    final leaves = Completer<void>();
    final call = TransportZone.run(
      leaves.future,
      () => _server(fake).beginRenewal('dev-1', KeyRole.ambient),
    );
    await pumpEventQueue();
    leaves.complete();
    await pumpEventQueue();
    gate.complete(_answer(200, {'challengeId': 'c-1', 'challenge': 'text'}));
    expect((await call).challenge, 'text');
    expect(fake.abortCount, 0);
  });

  test('unsignedPaths carry the base uri path and the base path', () {
    expect(
      DartVerifyDeviceAuthRestDeviceAuthServer.unsignedPaths(Uri.parse('https://api.example.com/v1')),
      [
        '/v1/api/\$procs/beginEnrolment',
        '/v1/api/\$procs/confirmEnrolment',
        '/v1/api/\$procs/beginRenewal',
        '/v1/api/\$procs/confirmRenewal',
      ],
    );
    expect(
      DartVerifyDeviceAuthRestDeviceAuthServer.unsignedPaths(
        Uri.parse('https://api.example.com/'),
        basePath: '',
      ).first,
      '/\$procs/beginEnrolment',
    );
  });
}
