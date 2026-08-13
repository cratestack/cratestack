import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';
import 'package:flutter_test/flutter_test.dart';

/// A fake [CratestackClientAdapter] recording every request — proves the
/// count of real network calls the test makes, exactly like
/// `boards_screen_test.dart`'s own fake does for `BoardApi`.
class _FakeAdapter implements CratestackClientAdapter {
  final requests = <CratestackRequest>[];

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    requests.add(request);
    return <String, Object?>{'totalMinutes': 75};
  }
}

void main() {
  // Regression test for issue #325: `estimateFocusMinutesProvider` is a
  // riverpod-generator "family" provider (its argument,
  // `EstimateFocusMinutesArgs`, comes from `client/`'s generated
  // `procedures.dart`). Riverpod's family cache dedupes provider
  // instances by *value* equality of the argument, not object identity.
  // Before this issue's fix, `EstimateFocusMinutesArgs` was a plain
  // generated Dart class with no `operator ==`/`hashCode` override, so
  // two structurally-identical instances were only `==` by identity —
  // which meant `board_detail_screen.dart` (this app's real consumer of
  // this provider) had to hand-memoize its argument object just to avoid
  // permanently restarting `AsyncLoading` on every rebuild (reproduced
  // live against a real server, see the issue). That memoization
  // workaround has since been deleted from `board_detail_screen.dart` —
  // this test is the proof it's safe to have deleted it: a *fresh*
  // `EstimateFocusMinutesArgs` instance, never `identical()` to the
  // first, must still hit the family provider's existing cache entry
  // rather than triggering a second network call or a fresh
  // `AsyncLoading`.
  test(
    'a fresh, value-equal EstimateFocusMinutesArgs instance reuses the '
    'already-resolved family provider entry instead of restarting '
    'AsyncLoading (issue #325)',
    () async {
      final fakeAdapter = _FakeAdapter();
      final container = ProviderContainer(
        overrides: [
          flutterRiverpodClientAdapterProvider.overrideWithValue(fakeAdapter),
        ],
      );
      addTearDown(container.dispose);

      final argsA = EstimateFocusMinutesArgs(
        args: FocusEstimateArgs(taskCount: 3, minutesPerTask: 25),
      );

      // Keeps this family member alive across the rest of the test — the
      // same role `ref.watch` plays in a real widget (see
      // `board_detail_screen.dart`'s `_FocusEstimate`). Without an active
      // listener, riverpod's default `autoDispose` would tear the
      // provider down the instant nothing watches it, which would mask
      // exactly the bug this test exists to catch (a still-referenced
      // rebuild losing its cache entry because of broken `==`, not a
      // provider that legitimately went out of scope).
      final subscription = container.listen(
        estimateFocusMinutesProvider(argsA),
        (previous, next) {},
      );
      addTearDown(subscription.close);

      final resultA = await container.read(
        estimateFocusMinutesProvider(argsA).future,
      );
      expect(resultA.totalMinutes, 75);
      expect(fakeAdapter.requests, hasLength(1));

      // A brand-new object — never `identical()` to `argsA` — but
      // structurally equal. This is exactly what `board_detail_screen.dart`
      // now constructs on every `build()` call (no memoization).
      final argsB = EstimateFocusMinutesArgs(
        args: FocusEstimateArgs(taskCount: 3, minutesPerTask: 25),
      );
      expect(
        identical(argsA, argsB),
        isFalse,
        reason: 'the test is meaningless if these are the same object',
      );
      expect(
        argsA,
        equals(argsB),
        reason:
            'dart_mappable\'s generated operator== must consider these equal '
            '— this is the fix issue #325 shipped',
      );

      // The regression this issue fixes: before `dart_mappable`,
      // riverpod's family cache keyed on `==`/`hashCode`, so `argsB` —
      // despite being value-equal — would have been a cache miss:
      // immediately `AsyncLoading` again and a second network call. That
      // is the literal "the estimate text never appeared, permanently
      // stuck in AsyncLoading" bug reproduced live against a server.
      final stateB = container.read(estimateFocusMinutesProvider(argsB));
      expect(
        stateB.hasValue,
        isTrue,
        reason:
            'a value-equal args instance should hit the family cache and '
            'already have data, not restart AsyncLoading',
      );
      expect(stateB.value?.totalMinutes, 75);
      expect(
        fakeAdapter.requests,
        hasLength(1),
        reason:
            'no second network call should have fired for the cache hit',
      );
    },
  );
}
