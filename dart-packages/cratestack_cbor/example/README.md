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

Supported platforms: **Linux desktop, Android, Windows desktop, macOS
desktop, iOS, and web**, matching the parent package's current platform
support. Linux arm64 has no folder here and cannot get one: this example is
verified by building and running a real Flutter app, and Flutter publishes no
arm64 Linux SDK on any channel, so there is no host on which
`flutter build linux` could produce an arm64 build to verify (see the parent
package's README for the release-manifest evidence).

`macos/` and `ios/` here are Flutter's own generated Xcode project
scaffolding only (`flutter create --platforms=macos .` /
`flutter create --platforms=ios .`) — neither includes a committed
`Podfile`. Generating one requires Xcode's project interpreter
(`_xcodeProjectInterpreter.isInstalled` gates `setupPodfile` in the Flutter
SDK's own `flutter_tools`), which this repo's own Linux-only dev toolchain
does not have — see `docs/tooling/cratestack-cbor-development.md` (repo
root) and `../macos/cratestack_cbor.podspec`'s/`../ios/cratestack_cbor
.podspec`'s header comments. A real macOS host running `flutter pub get`/
`flutter build macos`/`flutter build ios` generates the Podfile fresh from a
Flutter SDK template the first time it needs it — confirmed for `ios/` the
same way it was for `macos/`: `flutter create --platforms=ios .` ran
cleanly on this repo's Linux dev machine (Xcode-gated steps no-op instead of
failing) and produced a real `Runner.xcodeproj`/`Runner.xcworkspace`, just
no `Podfile`.

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
artifacts for the platform(s) you want first — see that package's README:

```bash
just cbor-vendor-native   # Linux + the flutter_rust_bridge Dart glue (needed by every other platform too)
just cbor-vendor-web
just cbor-vendor-android  # only if you're going to build for Android
just cbor-vendor-lib windows-x64   # only on Windows, only if you're going to build for Windows
just cbor-vendor-macos             # only on macOS, only if you're going to build for macOS
just cbor-vendor-ios               # only on macOS, only if you're going to build for iOS
cd example
flutter pub get
flutter run -d linux   # or: flutter run -d chrome / flutter run -d <android-device-id> / -d windows / -d macos / -d <ios-simulator-id>
```

## Real-build verification (not `flutter run`)

```bash
just cbor-example-verify           # Linux desktop + web, from the repo root
just cbor-example-verify-android   # Android APK build + per-ABI presence proof
just cbor-example-verify-windows   # Windows .exe build + DLL presence proof (must run ON Windows)
just cbor-example-verify-macos     # macOS .app build + universal xcframework presence proof (must run ON macOS)
just cbor-example-verify-ios       # iOS simulator .app build + xcframework presence proof (must run ON macOS)
```

`cbor-example-verify` builds this example for Linux desktop and web in
RELEASE mode, actually runs the Linux binary (headless, via `xvfb-run`) and
actually serves and loads the web release bundle in a real headless Chrome
(`tool/verify_web_console.dart`), and asserts both print the same CBOR-hex
round-trip result the parent package's own test suite asserts.

`cbor-example-verify-android` builds a real release APK
(`flutter build apk`) and asserts `libcratestack_client_flutter.so` actually
landed inside it for every claimed ABI (arm64-v8a, x86_64, armeabi-v7a) — a
build that merely compiles proves nothing about whether the Android plugin
scaffolding (`../android/build.gradle`) actually bundled the library. This
is what CI runs.

For the decisive on-device proof — the app actually round-tripping CBOR at
runtime, not just the library being present in the APK — there is a
**local/manual-only** companion (deliberately not wired into CI; booting an
Android emulator on a hosted runner is substantially heavier and flakier
than everything else this package's CI already does):

```bash
flutter emulators --launch <id>            # start a device/emulator first
just cbor-example-verify-android-emulator  # install + run it, assert the round-trip marker
```

See each `just` recipe's own comments in the repo's `justfile` for why
every step exists.
