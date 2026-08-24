// Runs under `flutter test` — the Dart VM, not a built app bundle. This
// still exercises the REAL `createCborCodec()`/native backend (via
// `Isolate.resolvePackageUri`'s dev-mode path — see
// `lib/src/native/native_cbor_codec.dart`), same as this package's own
// `dart test`. It is a fast sanity check, not a substitute for the real
// `flutter build linux` / `flutter build web` proof this example exists
// for — see this package's README for that verification.
import 'package:cratestack_cbor_example/main.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets(
    'round-trips a CBOR value through the real cratestack_cbor API and '
    'shows the result',
    (WidgetTester tester) async {
      // Started before the widget exists, exactly as `main()` does it — the
      // app takes the future rather than starting one on first build (see
      // `main.dart`'s comment, cratestack#704).
      await tester.pumpWidget(CratestackCborExampleApp(
        roundTrip: runRoundTrip(),
      ));
      await tester.pumpAndSettle();

      final resultFinder = find.byKey(const Key('cratestack_cbor_result'));
      expect(resultFinder, findsOneWidget);

      final text = tester.widget<Text>(resultFinder).data ?? '';
      expect(text, contains('ROUND-TRIP OK'));
      expect(text, isNot(contains('FAILED')));
    },
  );
}
