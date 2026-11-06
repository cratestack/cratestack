import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod_client/flutter_riverpod_client.dart';
import 'package:flutter_riverpod_example_app/src/session.dart';
import 'package:flutter_riverpod_example_app/src/step_up/clear_done_tasks.dart';
import 'package:flutter_riverpod_example_app/src/step_up/refusal_banner.dart';
import 'package:flutter_riverpod_example_app/src/step_up/step_up_screen.dart';
import 'package:flutter_test/flutter_test.dart';

/// Answers `clearDoneTasks` the way the server does under
/// `@authn(maxAge: 300, amr: ["hwk"])` (cratestack#1245): it reads the same
/// [DemoSession] the app's headers come from, and refuses a session that is
/// too old or has no `hwk` with the real step-up envelope, as a
/// `CratestackRestException` the generated adapter would raise for a 401.
class _StepUpServer implements CratestackClientAdapter {
  _StepUpServer(this.session);

  final DemoSession session;
  final clears = <CratestackRequest>[];

  @override
  Future<Object?> execute(
    CratestackRequest request, {
    CratestackCallOptions? options,
  }) async {
    if (request.path != r'/api/$procs/clearDoneTasks') {
      throw StateError('unexpected request: ${request.method} ${request.path}');
    }
    clears.add(request);
    final age = DateTime.now().difference(session.authTime).inSeconds;
    final unmet = [
      if (age > 300) 'max_age',
      if (!session.amr.contains('hwk')) 'amr',
    ];
    if (unmet.isNotEmpty) {
      throw CratestackRestException(
        status: 401,
        body: <String, Object?>{
          'code': 'UNAUTHORIZED',
          'message': 'step-up authentication required',
          'error': 'insufficient_user_authentication',
          'details': <String, Object?>{
            'max_age': 300,
            'amr': ['hwk'],
            'unmet': unmet,
          },
        },
      );
    }
    return <String, Object?>{'removed': 2};
  }
}

Future<_StepUpServer> _pump(WidgetTester tester) async {
  final session = DemoSession.stalePassword();
  final server = _StepUpServer(session);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        flutterRiverpodClientAdapterProvider.overrideWithValue(server)
      ],
      child: SessionScope(
        session: session,
        child: const MaterialApp(
          home: Scaffold(body: ClearDoneTasksButton(boardId: 1)),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Clear done tasks'));
  await tester.pumpAndSettle();
  return server;
}

void main() {
  testWidgets(
      'a stale password session is asked to step up, and the call is sent '
      'once more after the user confirms', (tester) async {
    final server = await _pump(tester);

    // The generated `StepUpRequired` drives the screen.
    expect(find.byType(StepUpScreen), findsOneWidget);
    expect(find.text('With: hwk'), findsOneWidget);
    expect(find.text('Missing now: max_age, amr'), findsOneWidget);
    expect(server.clears, hasLength(1));

    await tester.tap(find.text('Confirm with security key'));
    await tester.pumpAndSettle();

    expect(find.byType(StepUpScreen), findsNothing);
    expect(find.text('Removed 2 done tasks.'), findsOneWidget);
    expect(server.clears, hasLength(2), reason: 'one retry, no more');
    expect(find.byType(RefusalBanner), findsNothing);
  });

  testWidgets(
      'a declined step-up is not retried, and the banner names the refusal '
      'code', (tester) async {
    final server = await _pump(tester);

    await tester.tap(find.text('Not now'));
    await tester.pumpAndSettle();

    expect(server.clears, hasLength(1));
    expect(
      find.byKey(const ValueKey('refusal-insufficient_user_authentication')),
      findsOneWidget,
    );
    expect(find.textContaining('insufficient_user_authentication'),
        findsOneWidget);

    // The banner offers the step-up again.
    await tester.tap(find.text('Confirm now'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Confirm with security key'));
    await tester.pumpAndSettle();
    expect(find.text('Removed 2 done tasks.'), findsOneWidget);
    expect(server.clears, hasLength(3));
  });

  test('the banner is keyed on the refusal code, not only the status', () {
    const forbidden = CratestackRestException(
      status: 403,
      body: <String, Object?>{'code': 'FORBIDDEN', 'message': 'no'},
    );
    const attestation = CratestackRestException(
      status: 401,
      body: <String, Object?>{
        'code': 'UNAUTHORIZED',
        'message': 'expired',
        'error': 'attestation_expired',
      },
    );
    expect(refusalCode(forbidden), 'FORBIDDEN');
    expect(refusalCode(attestation), 'attestation_expired');
    expect(attestation.stepUp, isNull, reason: 'a 401 that is not a step-up');
    expect(refusalCode(StateError('offline')), isNull);
    expect(refusalText('FORBIDDEN'), 'You are not allowed to do this.');
  });
}
