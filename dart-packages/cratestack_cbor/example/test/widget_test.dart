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
      // The round trip seals and opens a COSE vector (cratestack#1026), and
      // those calls are asynchronous FFI: the native side answers on a real
      // port, which the fake-async zone `testWidgets` runs in never drains.
      // So the round trip runs in real time, before the widget exists, as
      // `main()` starts it (see `main.dart`'s comment, cratestack#704), and
      // the app is given the finished result.
      final result = await tester.runAsync(runRoundTrip);
      await tester.pumpWidget(CratestackCborExampleApp(
        roundTrip: Future.value(result!),
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
