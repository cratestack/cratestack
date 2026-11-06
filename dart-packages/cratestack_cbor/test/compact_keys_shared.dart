// The compact-wire-key assertions both backends must satisfy, written once
// and registered by `native_cbor_codec_test.dart` (vm) and
// `web_cbor_codec_test.dart` (browser) from inside their own `main`.
import 'dart:typed_data';

import 'package:cratestack_cbor/cratestack_cbor.dart';
import 'package:test/test.dart';

import 'shared_fixtures.dart';

Uint8List _bytes(String hex) => Uint8List.fromList(unhex(hex));

List<int> _list(Uint8List bytes) => bytes.toList();

void compactKeysTests() {
  group('createCompactKeys', () {
    late CompactKeys keys;

    setUpAll(() async {
      keys = await createCompactKeys(_bytes(compactKeysArtifactHex));
    });

    test('rewrites CBOR keys to integers and back', () {
      final named = _bytes(compactNamedHex);
      final compact = _bytes(compactCompactHex);
      expect(_list(keys.toCompact(named, 'shop.put', 'request')), _list(compact));
      expect(
        _list(keys.fromCompact(compact, 'shop.put', 'request')),
        _list(named),
      );
    });

    test('rewrites JSON keys to decimal strings and back', () {
      expect(
        keys.toCompactJson(compactNamedJson, 'shop.put', 'request'),
        compactCompactJson,
      );
      expect(
        keys.fromCompactJson(compactCompactJson, 'shop.put', 'request'),
        compactNamedJson,
      );
    });

    test('passes an opaque direction through untouched', () {
      final named = _bytes(compactNamedHex);
      expect(
        _list(keys.toCompact(named, 'shop.put', 'response')),
        _list(named),
      );
    });

    test('names the Cratestack-Contract selector, or null for an unknown op',
        () {
      expect(keys.selector('shop.put'), compactSelector);
      expect(keys.selector('shop.missing'), isNull);
    });

    test('the methods fit the generated hook signatures as tear-offs', () {
      final Uint8List Function(Uint8List, String, String) toCompact =
          keys.toCompact;
      final String? Function(String) selector = keys.selector;
      expect(toCompact(_bytes(compactNamedHex), 'shop.put', 'request'),
          isNotEmpty);
      expect(selector('shop.put'), compactSelector);
    });

    test('bad input throws CratestackCborCodecError, not a crash', () {
      final named = _bytes(compactNamedHex);
      expect(
        () => keys.toCompact(named, 'shop.missing', 'request'),
        throwsA(isA<CratestackCborCodecError>()),
      );
      expect(
        () => keys.toCompact(named, 'shop.put', 'sideways'),
        throwsA(isA<CratestackCborCodecError>()),
      );
      expect(
        () => keys.toCompact(Uint8List.fromList([0x1b]), 'shop.put', 'request'),
        throwsA(isA<CratestackCborCodecError>()),
      );
      expect(
        () => keys.fromCompactJson('{', 'shop.put', 'request'),
        throwsA(isA<CratestackCborCodecError>()),
      );
      expect(
        createCompactKeys(Uint8List.fromList([1, 2, 3])),
        throwsA(isA<CratestackCborCodecError>()),
      );
    });
  });
}
