import 'package:flutter/widgets.dart';

/// The demo's stand-in for a signed-in session: who the user is, when they
/// last authenticated and how (RFC 8176 `amr`). The reused `react-vite-swr`
/// server's `HeaderAuthProvider` reads them from `x-auth-id`, `x-auth-time`
/// and `x-auth-amr` and fills the principal contract's `auth_time` and `amr`
/// with them, which `clearDoneTasks`'s `@authn(maxAge: 300, amr: ["hwk"])`
/// checks.
///
/// Sending them as headers is a stand-in, like the existing `x-auth-id`: a
/// real app carries a credential (a token, a signed request) that the server
/// verifies, and the re-proof is its auth library's ceremony.
/// `examples/step-up-procedures` shows that with CrateStack's in-house
/// provider.
class DemoSession {
  DemoSession(
      {required this.userId, required this.authTime, required this.amr});

  /// A password sign-in twenty minutes ago: enough for every board and
  /// task operation, too old and too weak for `clearDoneTasks`.
  factory DemoSession.stalePassword({DateTime? now}) => DemoSession(
        userId: 1,
        authTime: (now ?? DateTime.now()).subtract(const Duration(minutes: 20)),
        amr: const ['pwd'],
      );

  final int userId;
  DateTime authTime;
  List<String> amr;

  /// The re-proof: the user confirmed with the device's hardware key, now.
  /// A real app runs its auth library's ceremony here (a user-present key
  /// signature, a passkey) and gets a credential that says so.
  void stepUp({DateTime? now}) {
    authTime = now ?? DateTime.now();
    amr = const ['hwk', 'user'];
  }

  /// The headers every request carries.
  Map<String, String> headers() => <String, String>{
        'x-auth-id': '$userId',
        'x-auth-time': '${authTime.millisecondsSinceEpoch ~/ 1000}',
        'x-auth-amr': amr.join(' '),
      };
}

/// Hands the [DemoSession] to the step-up screen, which updates it.
class SessionScope extends InheritedWidget {
  const SessionScope({super.key, required this.session, required super.child});

  final DemoSession session;

  static DemoSession of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<SessionScope>();
    assert(scope != null, 'no SessionScope above this widget');
    return scope!.session;
  }

  @override
  bool updateShouldNotify(SessionScope oldWidget) =>
      session != oldWidget.session;
}
