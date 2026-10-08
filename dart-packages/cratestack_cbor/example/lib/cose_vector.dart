// One shared COSE vector (`crates/cratestack-cose/tests/vectors/unary.json`:
// `rpc-request-sign1-ed25519-cti16` and its response
// `rpc-response-sign1-ed25519`), checked at app start the way the codec's
// fixture is, so a built app proves it can seal and open (cratestack#1026),
// not only that the package's tests do.
import 'dart:typed_data';

import 'package:cratestack_cbor/cose.dart';
import 'package:cratestack_cbor/cose_testing.dart';

Uint8List _unhex(String text) => Uint8List.fromList([
      for (var i = 0; i < text.length; i += 2)
        int.parse(text.substring(i, i + 2), radix: 16),
    ]);

String _hex(List<int> bytes) =>
    bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

const _payload =
    'a7626964500192f5a87c3e7b219d4f6a1e2c3b4d5e6570617965726d416d696e61205463'
    '686f75706f66616d6f756e741a0001e8486863757272656e6379635841466673746174757367'
    '736574746c65646a637265617465645f61741a6ab13b80646e6f74656972656e742073657074';

/// The sealed request the vector pins.
const expectedSealedHex =
    'd2845827a301320448be5de2f4bcdc383a0fa2061a6ab13b8007503c9a5e71d20b48f6a1c7e4029b6d5f83a05870'
    'a7626964500192f5a87c3e7b219d4f6a1e2c3b4d5e6570617965726d416d696e61205463686f75706f66616d6f75'
    '6e741a0001e8486863757272656e6379635841466673746174757367736574746c65646a637265617465645f6174'
    '1a6ab13b80646e6f74656972656e742073657074584068a4d23aa86784147bb8b16ee3d211e9640276f91605e3f9'
    'fccacb55030e7035683022a74d972b171c8b60a3eb7167a4a7fa72acdb2a71e36c8a46158ba27c06';

const _response =
    'd2844da201320448be5de2f4bcdc383aa05870a7626964500192f5a87c3e7b219d4f6a1e2c3b4d5e6570617965726d'
    '416d696e61205463686f75706f66616d6f756e741a0001e8486863757272656e6379635841466673746174757367'
    '736574746c65646a637265617465645f61741a6ab13b80646e6f74656972656e742073657074584077c5e17de4d8'
    'a71b0fbb8aa8fcf421b6f9efbdc442f944b989ed6798473fed8ff266d24c6b0af0243839a678cea6e0f542f0cf511'
    'b672802777bc387b8326404';

/// Seals the vector's request with its pinned `iat` and `cti`, compares the
/// bytes with the vector, then opens the vector's response. Returns the
/// sealed request's hex; throws if either half differs.
Future<String> runCoseVector() async {
  final envelope = await ClientEnvelopeForVectors.forVectors(
    signer: Ed25519Signer.fromSeed(
      Uint8List.fromList([for (var i = 0; i < 32; i++) i]),
    ),
    serverKeys: [
      CoseServerKey.ed25519(
        _unhex(
          '03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8',
        ),
      ),
    ],
    audience: 'payments',
    iat: 1790000000,
    cti: _unhex('3c9a5e71d20b48f6a1c7e4029b6d5f83'),
  );
  final binding = CallBinding(
    method: 'POST',
    route: 'model.Payment.create',
    contractSha: _unhex(
      'a73857a1ee92de06939047d9544afefd554ab8eb8df6c91dcd740ca875f0a56a',
    ),
  );
  final sealed = await envelope.sealRequest(_unhex(_payload), binding);
  if (_hex(sealed) != expectedSealedHex) {
    throw StateError('the sealed request differs from the shared vector');
  }
  final opened = await envelope.openResponse(
    _unhex(_response),
    binding: binding,
    sealedRequest: sealed,
    status: 200,
  );
  if (_hex(opened.payload) != _payload) {
    throw StateError('the opened response differs from the shared vector');
  }
  return _hex(sealed);
}
