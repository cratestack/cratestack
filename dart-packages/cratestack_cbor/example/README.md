# cratestack_cbor_example

A minimal Flutter app that exercises `package:cratestack_cbor` end to end
(cratestack#563's "Flutter app integration" slice). This is not a demo of
UI patterns — it exists specifically to prove `cratestack_cbor` works
inside a REAL `flutter build`, not just `dart test`. See the parent
package's README and `docs/tooling/cratestack-cbor-development.md` (repo
root) for the full story of why that distinction matters.

`lib/main.dart` calls the real, public `createCborCodec()` API at startup,
round-trips a JSON value through it, shows the result on screen, and
prints it to stdout with a `CRATESTACK_CBOR_EXAMPLE_RESULT:` marker — the
marker is load-bearing, not decorative: a built desktop binary or a served
web bundle has no interactive console to read, so verification greps for
it (`../README.md` and this repo's `just cbor-example-verify`) instead of
trying to screenshot a GUI.

Supported platforms: **Linux desktop and web only**, matching the parent
package's current platform support. Android and iOS platform folders were
deliberately not generated here — out of scope for cratestack#563's
current slice (see the parent package's README).

## Why this lives under `example/` inside the package

Dart/Flutter convention is an `example/` directory inside the package
itself (it also feeds the pub.dev score once `cratestack_cbor` is
published) — chosen deliberately over a sibling directory under
`examples/` at the repo root, unlike `examples/flutter-riverpod` or
`examples/embedded-flutter`. Those are full generator-driven demos wired
to a real backend server; this is a package-level smoke test with no
server dependency, so the pub.dev-facing `example/` convention is the
better fit.

## Running it yourself

From `dart-packages/cratestack_cbor/` (the parent package), vendor the
native + web artifacts first — see that package's README:

```bash
just cbor-vendor-native
just cbor-vendor-web
cd example
flutter pub get
flutter run -d linux   # or: flutter run -d chrome
```

## Real-build verification (not `flutter run`)

```bash
just cbor-example-verify   # from the repo root
```

builds this example for Linux desktop and web in RELEASE mode, actually
runs the Linux binary (headless, via `xvfb-run`) and actually serves and
loads the web release bundle in a real headless Chrome
(`tool/verify_web_console.dart`), and asserts both print the same
CBOR-hex round-trip result the parent package's own test suite asserts.
See that `just` recipe's own comments in the repo's `justfile` for why
each step exists.
