// Bounded capture of a subprocess's stderr for `verify_web_console.dart`
// (cratestack CI flake: Chrome DevTools readiness timing out with zero
// diagnostics — see the parent script's module doc). Split out on its own
// so `devtools_ready.dart` and any self-test can share it without pulling
// in process-launching or DevTools-polling concerns.
//
// Keeps only the most recent [maxBytes] so a misbehaving Chrome flooding
// stderr cannot grow this without bound over a 60s readiness window.
class ChromeStderrCapture {
  ChromeStderrCapture({this.maxBytes = 4096});

  final int maxBytes;
  final StringBuffer _buffer = StringBuffer();

  void add(String chunk) {
    _buffer.write(chunk);
    final current = _buffer.toString();
    if (current.length > maxBytes) {
      final trimmed = current.substring(current.length - maxBytes);
      _buffer
        ..clear()
        ..write(trimmed);
    }
  }

  /// The captured tail, or a placeholder if nothing was ever written —
  /// callers embed this directly in a diagnostic message, and "(empty)" is
  /// more useful there than a blank line that looks like a formatting bug.
  String get tail {
    final text = _buffer.toString();
    return text.isEmpty ? '(empty)' : text;
  }
}
