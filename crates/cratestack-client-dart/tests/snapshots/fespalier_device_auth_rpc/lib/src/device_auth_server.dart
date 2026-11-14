// The device ceremonies of this schema as fespalier's `DeviceAuthServer`
// (cratestack#1303): the procedures marked with the device-auth attribute,
// called through the generated client.
//
// `fespalier_sign_keypair` knows the order of the legs, what is signed and
// what is kept; it knows no URL and no payload. This class is the one place
// that does. Hand it the transport you gave `AuthConfig.transport` (the plain
// chain, with NO session on it, so the four legs go out unsigned by
// construction):
//
//     final backend = DeviceAuthBackend(
//       server: DeviceAuthRpcClientDeviceAuthServer(
//         transport,
//         baseUri: Uri.parse('https://api.example.com'),
//         fieldErrors: errors.fieldErrorsOf, // optional: a 422 as a form's FieldErrors
//       ),
//       keys: DeviceKeys.device(),
//       profile: const DeviceSignatureProfile(audience: 'https://api.example.com'),
//     );
//
// A 403 on a renewal (or on `confirmEnrolment`) is an `AuthRejected`; any
// other failure is thrown as it is. The `fespalier_*` packages it imports are
// git dependencies pinned in this package's pubspec at `v0.21.0`.

import 'dart:async';

import 'package:fespalier_auth/fespalier_auth.dart' show AuthRejected, AuthUser;
import 'package:fespalier_keypair/fespalier_keypair.dart' show KeyRole;
import 'package:fespalier_sign_keypair/fespalier_sign_keypair.dart'
    show
        Attestation,
        DeviceAuthServer,
        DeviceCredential,
        DeviceEnrolment,
        DeviceSigner,
        EnrolmentChallenge,
        EnrolmentStart,
        RoleChallenge;
import 'package:fespalier_transport/fespalier_transport.dart'
    show HttpTransport, TransportRequest, TransportResponse, TransportZone;

import '../device_auth_rpc_client.dart';

/// The device-auth procedures of this schema, as a `DeviceAuthServer`.
///
/// Built over [transport]: `AuthConfig.transport`, the chain with no session
/// on it. If the legs go through `authTransport` instead, list
/// [unsignedPaths] in `DeviceAuthBackend.unsignedPaths`.
final class DeviceAuthRpcClientDeviceAuthServer implements DeviceAuthServer {
  /// The device endpoints of the server at [baseUri], reached through
  /// [transport] (no session).
  DeviceAuthRpcClientDeviceAuthServer(
    this.transport, {
    required Uri baseUri,
    this.fieldErrors,
    this.userOf,
  }) : _baseUri = baseUri;

  /// What the legs go through: `AuthConfig.transport`.
  final HttpTransport transport;

  /// Turns an error answer into what a form shows under its field (a wrong
  /// number or code is a `422`): pass `errors.fieldErrorsOf` of
  /// `fespalier_cratestack`'s `CrateStackErrors`. When it returns something,
  /// that is thrown; with no callback, or a `null`, the error is thrown as it
  /// is.
  final Object? Function(Object error)? fieldErrors;

  /// Builds the signed-in user from the answer of `confirmEnrolment`, in place of the
  /// default: `userId` is the id, `name` and `email` are those fields when the
  /// answer has them, and every other field is in `claims`.
  final AuthUser Function(EnrolledDevice answer)? userOf;

  final Uri _baseUri;
  late final ProceduresApi _procedures = _proceduresOver(transport);

  ProceduresApi _proceduresOver(HttpTransport over) => DeviceAuthRpcClientCratestackClient(
    CratestackRpcHttpAdapter(
      CratestackFespalierTransport(over),
      baseUri: _baseUri,
    ),
  ).procedures;

  /// The `request.uri.path` of each leg that goes out with no session, for
  /// `DeviceAuthBackend(unsignedPaths: ...)`, when the legs go through a
  /// transport that signs (`authTransport`) and not through
  /// `AuthConfig.transport`.
  static List<String> unsignedPaths(Uri baseUri) {
    final prefix = baseUri.path.endsWith('/')
        ? baseUri.path.substring(0, baseUri.path.length - 1)
        : baseUri.path;
    return <String>[
      '$prefix/rpc/procedure.sendEnrolmentCode',
      '$prefix/rpc/procedure.beginEnrolment',
      '$prefix/rpc/procedure.confirmEnrolment',
      '$prefix/rpc/procedure.beginRenewal',
      '$prefix/rpc/procedure.confirmRenewal',
    ];
  }

  /// Asks the server to send the one-time code, before leg one: for a server
  /// that checks the code before it issues a challenge (Authkestra does), so
  /// the code then goes with the keys, as a field of `EnrolmentStart.fields`
  /// in [beginEnrolment]. Answers what `sendEnrolmentCode` answers.
  Future<CodeSent> sendEnrolmentCode(SendEnrolmentCodeArgs args) =>
      _leg(() => _procedures.sendEnrolmentCode(args));

  /// Leg one: the public keys of [EnrolmentStart.keys] and the app's own
  /// [EnrolmentStart.fields] (read by name, as text), answered with one
  /// challenge per key and the rest of the answer in
  /// [EnrolmentChallenge.data].
  @override
  Future<EnrolmentChallenge> beginEnrolment(EnrolmentStart start) async {
    final answer = await _leg(
      () => _procedures.beginEnrolment(
        BeginEnrolmentProcedureArgs(
          args: BeginEnrolmentArgs(
            phone: '${start.fields['phone'] ?? ''}',
            otp: '${start.fields['otp'] ?? ''}',
            ambient: _jwk(start.keys[KeyRole.ambient]!),
            userPresent: switch (start.keys[KeyRole.userPresent]) { final jwk? => _jwk(jwk), null => null },
          ),
        ),
      ),
    );
    return EnrolmentChallenge(
      challenges: {
        KeyRole.ambient: answer.ambient,
        KeyRole.userPresent: ?answer.userPresent,
      },
      data: {
        'enrolmentId': answer.enrolmentId,
      },
    );
  }

  /// Leg two: the signatures of the challenges, with what leg one answered
  /// (read back from [EnrolmentChallenge.data]).
  /// [proof] is not read: the one-time code went with leg one.
  /// A 403 is an `AuthRejected`.
  @override
  Future<DeviceEnrolment> confirmEnrolment(
    EnrolmentChallenge challenge,
    Map<KeyRole, String> challengeSignatures,
    Object? proof,
  ) async {
    final answer = await _leg(
      () => _procedures.confirmEnrolment(
        ConfirmEnrolmentProcedureArgs(
          args: ConfirmEnrolmentArgs(
            enrolmentId: '${challenge.data['enrolmentId'] ?? ''}',
            ambient: challengeSignatures[KeyRole.ambient]!,
            userPresent: challengeSignatures[KeyRole.userPresent],
          ),
        ),
      ),
      rejectedOn403: true,
    );
    return DeviceEnrolment(
      deviceId: answer.deviceId,
      attestations: {
        KeyRole.ambient: Attestation.parse(answer.ambient),
        if (answer.userPresent case final jws?) KeyRole.userPresent: Attestation.parse(jws),
      },
      user: userOf?.call(answer) ??
          AuthUser(
            id: answer.userId,
            claims: {
              'phone': answer.phone,
            },
          ),
    );
  }

  /// Leg one of a renewal: a challenge for the key of [role] of [deviceId].
  /// A 403 is an `AuthRejected`: the device is dead.
  @override
  Future<RoleChallenge> beginRenewal(String deviceId, KeyRole role) async {
    final answer = await _leg(
      () => _procedures.beginRenewal(
        BeginRenewalProcedureArgs(
          args: BeginRenewalArgs(
            deviceId: deviceId,
            role: role.name,
          ),
        ),
      ),
      rejectedOn403: true,
    );
    return RoleChallenge(
      role: role,
      challenge: answer.challenge,
      data: {
        'challengeId': answer.challengeId,
      },
    );
  }

  /// Leg two of a renewal: the [signature] of the challenge, answered with a
  /// new attestation for the same key. A 403 is an `AuthRejected`.
  @override
  Future<Attestation> confirmRenewal(
    RoleChallenge challenge,
    String signature,
  ) async {
    final answer = await _leg(
      () => _procedures.confirmRenewal(
        ConfirmRenewalProcedureArgs(
          args: ConfirmRenewalArgs(
            challengeId: '${challenge.data['challengeId'] ?? ''}',
            signature: signature,
          ),
        ),
      ),
      rejectedOn403: true,
    );
    return Attestation.parse(answer.attestation);
  }

  /// Tells the server this device is gone, at sign-out. This call is signed by
  /// the device, not by a session (the session is ending), so it goes through
  /// a transport that signs it with [signer]: `DeviceAuthBackend`'s, which
  /// asks for the user-present key when the device has one, and that prompts.
  /// A refused prompt or an unreachable server throws, and the sign-out goes on.
  @override
  Future<void> revoke(DeviceCredential credential, DeviceSigner signer) async {
    final signed = _proceduresOver(_SignedBy(transport, credential, signer));
    await _leg(
      () => signed.revokeDevice(
        RevokeDeviceArgs(
          args: RevokeArgs(
            deviceId: credential.deviceId,
          ),
        ),
      ),
    );
  }

  /// One call of a ceremony, mapped. A `403` is the server saying no, which
  /// [rejectedOn403] makes an [AuthRejected] (the device is unknown or
  /// revoked, and the session is over). Anything [fieldErrors] recognises (a
  /// `422` on a wrong code or number) is thrown as what it returns, for a
  /// form to show under its field. Anything else (offline, a `5xx`) is
  /// thrown as it is: "could not ask", and the session stays.
  ///
  /// The call runs in a zone with **no abort trigger of the caller's**: a
  /// renewal is shared by every request that waits for it, and one page
  /// leaving (`ref.cancellable` puts its trigger in the zone of the load it
  /// started) might abort it for the others. That is a **suspected**
  /// `fespalier_auth` issue, read in its source and neither reproduced nor
  /// confirmed; this guards against it until fespalier says it cannot happen.
  Future<T> _leg<T>(
    Future<T> Function() call, {
    bool rejectedOn403 = false,
  }) async {
    final neverAborted = Completer<void>().future;
    try {
      return await TransportZone.run(neverAborted, call);
    } on CratestackRpcException catch (error) {
      if (rejectedOn403 && error.status == 403) {
        throw const AuthRejected();
      }
      final mapped = fieldErrors?.call(error);
      if (mapped != null) throw mapped;
      rethrow;
    }
  }

  /// A public key of [EnrolmentStart.keys] as a `Jwk`.
  static Jwk _jwk(Map<String, String> jwk) => Jwk(
    kty: jwk['kty']!,
    crv: jwk['crv']!,
    x: jwk['x']!,
    y: jwk['y']!,
  );
}

/// A transport that signs every request with [signer] for [credential] before
/// it sends it on: how `revoke` is signed by the device.
final class _SignedBy implements HttpTransport {
  const _SignedBy(this.inner, this.credential, this.signer);

  final HttpTransport inner;
  final DeviceCredential credential;
  final DeviceSigner signer;

  @override
  Future<TransportResponse> send(TransportRequest request) async {
    final signature = await signer.signFor(credential, request);
    return inner.send(
      request.copyWith(headers: {...request.headers, ...signature}),
    );
  }
}
