// A replay oracle standing in for a keystore, so the four ESP256 request
// vectors run through the Dart API with no crypto in Dart.
//
// The vector signatures are RFC 6979 and low-`s`, so a callback that returns
// the vector's own signature for the vector's own to-be-signed bytes is a
// keystore that signs exactly as the vector's key does. It asserts nothing
// itself (a throw inside a callback would surface as a signer failure);
// [Esp256Oracle.seen] records what it was asked to sign, for the test to
// compare after the seal.
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';

/// How the fake keystore spells the signature it returns.
enum Spelling {
  /// 64 bytes, `r || s`, low-`s`: the vector's own bytes.
  raw,

  /// The same signature, DER encoded, as Android and iOS return it.
  der,

  /// `r || (n - s)`: a valid signature, which the envelope must normalise
  /// back to the vector's low-`s` bytes.
  highS,

  /// The high-`s` signature, DER encoded.
  highSDer,
}

/// The P-256 group order.
final BigInt p256Order = BigInt.parse(
  'ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551',
  radix: 16,
);

BigInt _int(List<int> bytes) =>
    bytes.fold(BigInt.zero, (acc, b) => (acc << 8) | BigInt.from(b));

Uint8List bytes32(BigInt value) {
  final out = Uint8List(32);
  for (var i = 31; i >= 0; i--) {
    out[i] = (value & BigInt.from(0xff)).toInt();
    value >>= 8;
  }
  return out;
}

Uint8List _derInt(Uint8List bytes) {
  var start = 0;
  while (start < bytes.length - 1 && bytes[start] == 0) {
    start++;
  }
  final body = bytes.sublist(start);
  final pad = body[0] & 0x80 != 0;
  return Uint8List.fromList([
    0x02,
    body.length + (pad ? 1 : 0),
    if (pad) 0,
    ...body,
  ]);
}

Uint8List _der(Uint8List raw) {
  final body = [..._derInt(raw.sublist(0, 32)), ..._derInt(raw.sublist(32))];
  return Uint8List.fromList([0x30, body.length, ...body]);
}

/// [raw] (64 bytes) spelled as [spelling].
Uint8List spell(Uint8List raw, Spelling spelling) {
  final high = switch (spelling) {
    Spelling.highS || Spelling.highSDer => Uint8List.fromList([
        ...raw.sublist(0, 32),
        ...bytes32(p256Order - _int(raw.sublist(32))),
      ]),
    _ => raw,
  };
  return switch (spelling) {
    Spelling.der || Spelling.highSDer => _der(high),
    _ => high,
  };
}

/// An [ExternalSigner] for one vector request, replaying its signature.
final class Esp256Oracle {
  Esp256Oracle({
    required Uint8List publicKey,
    required Uint8List coseMessage,
    Spelling spelling = Spelling.raw,
  }) {
    final signature = coseMessage.sublist(coseMessage.length - 64);
    signer = ExternalSigner.esp256(
      publicKey: publicKey,
      timeout: const Duration(seconds: 30),
      sign: (tbs) async {
        seen.add(Uint8List.fromList(tbs));
        return SignOutcome.signature(spell(signature, spelling));
      },
    );
  }

  /// The signer to seal with.
  late final ExternalSigner signer;

  /// The bytes the envelope asked the keystore to sign, one entry per call.
  final List<Uint8List> seen = [];
}
