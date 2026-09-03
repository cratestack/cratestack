// CLI option parsing for `verify_web_console.dart`. Split out unchanged in
// contract (every existing flag keeps its name, default and meaning) so
// the readiness-flake fix (`devtools_ready.dart`) doesn't have to touch
// argument handling beyond adding the two new options below.
import 'dart:io';

class Options {
  Options({
    required this.url,
    required this.timeoutSeconds,
    required this.debugPort,
    required this.expectFailure,
    required this.devtoolsReadyTimeoutSeconds,
    required this.hardTimeoutSeconds,
    this.expectHex,
  });

  final String url;
  final int timeoutSeconds;
  final int debugPort;
  final bool expectFailure;
  final String? expectHex;

  /// How long to wait for Chrome's DevTools Protocol to answer
  /// `/json/version` before giving up (and, via `devtools_ready.dart`,
  /// retrying once). Defaults to 60s — the old hardcoded 15s deadline was
  /// the root cause of a repeated CI flake (see `devtools_ready.dart`'s
  /// module doc). Overridable per-run without touching the default via
  /// `--devtools-ready-timeout-seconds` or the
  /// `CRATESTACK_CBOR_DEVTOOLS_READY_SECONDS` environment variable
  /// (the flag wins if both are given), mirroring how `CHROME_EXECUTABLE`
  /// already lets this script's Chrome binary be overridden.
  final int devtoolsReadyTimeoutSeconds;

  /// Absolute wall-clock ceiling on this whole tool's lifetime, enforced
  /// by `HardTimeoutWatchdog` regardless of what else it is doing.
  /// Defaults to 180s. Overridable via `--hard-timeout-seconds` or
  /// `CRATESTACK_CBOR_HARD_TIMEOUT_SECONDS` (flag wins). See
  /// `hard_timeout_watchdog.dart`'s module doc for why this exists: a
  /// prior version of this script relied on the Dart event loop draining
  /// naturally and hung for 45 minutes in CI when that assumption broke.
  final int hardTimeoutSeconds;

  static Options parse(List<String> args) {
    String? url;
    var timeoutSeconds = 15;
    var debugPort = 9333;
    var expectFailure = false;
    String? expectHex;
    int? devtoolsReadyTimeoutSeconds;
    int? hardTimeoutSeconds;
    for (var i = 0; i < args.length; i++) {
      switch (args[i]) {
        case '--url':
          url = args[++i];
        case '--timeout-seconds':
          timeoutSeconds = int.parse(args[++i]);
        case '--debug-port':
          debugPort = int.parse(args[++i]);
        case '--expect-failure':
          expectFailure = true;
        case '--expect-hex':
          expectHex = args[++i];
        case '--devtools-ready-timeout-seconds':
          devtoolsReadyTimeoutSeconds = int.parse(args[++i]);
        case '--hard-timeout-seconds':
          hardTimeoutSeconds = int.parse(args[++i]);
        default:
          throw ArgumentError('unknown argument: ${args[i]}');
      }
    }
    if (url == null) {
      throw ArgumentError('--url is required');
    }
    return Options(
      url: url,
      timeoutSeconds: timeoutSeconds,
      debugPort: debugPort,
      expectFailure: expectFailure,
      expectHex: expectHex,
      devtoolsReadyTimeoutSeconds: devtoolsReadyTimeoutSeconds ??
          _intFromEnv('CRATESTACK_CBOR_DEVTOOLS_READY_SECONDS', 60),
      hardTimeoutSeconds: hardTimeoutSeconds ??
          _intFromEnv('CRATESTACK_CBOR_HARD_TIMEOUT_SECONDS', 180),
    );
  }

  static int _intFromEnv(String name, int defaultValue) {
    final raw = Platform.environment[name];
    if (raw == null) {
      return defaultValue;
    }
    final parsed = int.tryParse(raw);
    if (parsed == null) {
      throw ArgumentError('$name must be an integer, got "$raw"');
    }
    return parsed;
  }
}
