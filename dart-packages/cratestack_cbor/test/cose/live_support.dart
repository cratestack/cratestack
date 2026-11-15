// Support for `live_test.dart`: the example server process, the published
// test keys, and a software keystore.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';

import 'software_keystore.dart';

const audience = 'roundtrip';

Uint8List seed(int start) =>
    Uint8List.fromList([for (var i = 0; i < 32; i++) start + i]);

Uint8List unhex(String text) => Uint8List.fromList([
      for (var i = 0; i < text.length; i += 2)
        int.parse(text.substring(i, i + 2), radix: 16),
    ]);

/// `ed25519_other` of `keys.json`: the public half of the server's seed.
final serverPublic = unhex(
  'cd14b37f956e953194ff7fb73b3d81dcc561d61a7538094b7c3e1a643ee5f3aa',
);

/// One running example server.
class Server {
  Server(this.process, this.port, this.contracts);

  final Process process;
  final int port;

  /// Route to op digest, as the server printed them.
  final Map<String, Uint8List> contracts;

  static Future<Server> start(String executable, String mode) async {
    final process = await Process.start(executable, ['--mode', mode]);
    final contracts = <String, Uint8List>{};
    final ready = Completer<int>();
    process.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) {
      final parts = line.split(' ');
      if (parts.first == 'contract' && parts.length == 4) {
        contracts[parts[2]] = unhex(parts[3]);
      } else if (parts.first == 'listening') {
        ready.complete(int.parse(parts[1]));
      }
    });
    unawaited(process.stderr.drain<void>());
    final port = await ready.future.timeout(const Duration(seconds: 30));
    return Server(process, port, contracts);
  }

  void stop() => process.kill();
}

/// What a client of `kind` signs with, and the server (`mode`) it trusts.
///
/// `esp256` is a software P-256 key behind an [ExternalSigner], standing in
/// for a platform keystore: the callback gets the to-be-signed bytes and
/// answers with a DER signature, as Android and iOS do. The server (mode
/// `sign1`) trusts that key and still signs its answers with Ed25519.
({CoseSigner signer, CoseServerKey server, int alg}) credentials(
  String mode,
  String kind,
) {
  if (kind == 'esp256') {
    final keystore = SoftwareKeystore(p256Scalar);
    return (
      signer: ExternalSigner.esp256(
        publicKey: keystore.publicKey,
        timeout: const Duration(seconds: 30),
        sign: (tbs) async => SignOutcome.signature(keystore.signDer(tbs)),
      ),
      server: CoseServerKey.ed25519(serverPublic),
      alg: -9,
    );
  }
  if (mode == 'sign1') {
    return (
      signer: Ed25519Signer.fromSeed(seed(0)),
      server: CoseServerKey.ed25519(serverPublic),
      alg: -19,
    );
  }
  final secret = seed(0x40);
  return (
    signer: HmacSigner(CoseAlg.hmac256x64, secret),
    server: CoseServerKey.hmac(CoseAlg.hmac256x64, secret),
    alg: 4,
  );
}

/// `p256` of `keys.json`: the private scalar the example server trusts the
/// public half of. Published in this repository, so never a real key.
const p256Scalar =
    'c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721';

/// `p256`'s `kid` in `keys.json`: the RFC 9679 thumbprint prefix of its
/// public key.
const p256Kid = '750ca40795ab23e5';
