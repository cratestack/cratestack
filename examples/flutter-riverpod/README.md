# flutter-riverpod example

The epic #297 payoff: a real, running Flutter app that consumes a `cratestack generate-dart --preset
riverpod` client with **zero hand-written providers** — every `ref.watch(boardListProvider)` /
`ref.read(boardCreateControllerProvider.notifier).create(...)` call in `app/lib` comes from the
generator, not from this app.

## What it shows

- **A generated client, checked in** (`client/`) — run through the real CLI, not hand-written, and
  regenerable at any time (see below). One `@riverpod` provider per operation: `board`/`boardList`
  (reads), `BoardCreateController`/`BoardUpdateController`/`BoardDeleteController` (writes), same
  shape for `Task`, plus `estimateFocusMinutes` for the procedure.
- **A real Flutter app** (`app/lib`) that only imports generated code:
  - `BoardsScreen` — `boardListProvider` (a list read) and `boardCreateControllerProvider` (a
    mutation), with no manual refetch call anywhere in the file — just `ref.invalidate
    (boardListProvider)` after a write, which is ordinary Riverpod usage, not a hand-written provider.
  - `BoardDetailScreen` — `board(id)`, `taskList` (filtered client-side by `boardId` — the generated
    `list()` has no server-side filter yet, same known gap `react-vite-swr`'s README documents for its
    TypeScript sibling), the three `Task*Controller`s, and `estimateFocusMinutes` — a query-kind
    procedure provider that recomputes automatically as the open-task count changes.
- **Overrides the adapter provider to point at a real local server** — `flutterRiverpodClientAdapterProvider.overrideWithValue(CratestackDioAdapter(dio: myDio))` in `app/lib/main.dart`, the one and only override every consumer of this preset needs (see `client/README.md`'s "Riverpod Setup").
- **Reuses `react-vite-swr`'s schema and server** rather than standing up a second Postgres-backed
  crate: `examples/react-vite-swr/schema.cstack` (`Board`/`Task` models with a relation, plus an
  `estimateFocusMinutes` procedure) and `cargo run -p react-vite-swr-example` are the "existing example
  schema" this story's acceptance criteria ask to prefer.

## The full path: schema → generate → build_runner → run

```bash
# 1. Bring up Postgres (repo root)
docker compose up -d postgres

# 2. Run the server (reused from react-vite-swr — this example owns no Rust crate of its own)
DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test \
  cargo run -p react-vite-swr-example
# -> react-vite-swr-server listening on http://127.0.0.1:3210 (routes under /api)

# 3. Generate the client (from the repo root) — this is what produced client/, and what regenerates
#    it after a schema change. `client/` is committed (minus build_runner's own `.g.dart` output —
#    see client/.gitignore) so a fresh clone only needs step 4, not a Rust toolchain, to build the app.
#    --run-build-runner (issue #303) does both generation AND `dart run build_runner build
#    --delete-conflicting-outputs` in one command:
cargo run -p cratestack-cli -- generate-dart \
  --schema examples/react-vite-swr/schema.cstack \
  --out examples/flutter-riverpod/client \
  --library-name flutter_riverpod_client \
  --preset riverpod \
  --run-build-runner

# Without the flag, the equivalent two steps:
#   cargo run -p cratestack-cli -- generate-dart --schema ... --out ... --library-name ... --preset riverpod
#   (cd examples/flutter-riverpod/client && dart run build_runner build --delete-conflicting-outputs)

# (Drift check instead of regenerating — used by CI)
cargo run -p cratestack-cli -- generate-dart \
  --schema examples/react-vite-swr/schema.cstack \
  --out examples/flutter-riverpod/client \
  --library-name flutter_riverpod_client \
  --preset riverpod \
  --check

# 4. Materialize the app's platform scaffolds (android/, ios/, macos/ are gitignored and regenerated
#    locally — same precedent ../embedded-flutter set — `.` keeps the existing pubspec.yaml + lib/):
cd examples/flutter-riverpod/app
flutter create . --org dev.cratestack.examples --platforms=macos,ios,android
flutter pub get

# 5. Run it (macOS desktop shown; swap -d for ios/android/chrome)
flutter run -d macos
```

Add a board, tap it, add/toggle/delete a task — watch the list refresh with no manual fetch call
anywhere in `boards_screen.dart`/`board_detail_screen.dart`, and the focus-time estimate recompute as
the open-task count changes.

## Layout

| Path | What |
|---|---|
| [`client/`](client) | Generated `--preset riverpod` package (checked in; `.g.dart` build_runner output is not — see `client/.gitignore`) |
| [`app/`](app) | The Flutter app — `lib/main.dart` (the one adapter override), `lib/src/screens/`, `test/boards_screen_test.dart` |

`../react-vite-swr/schema.cstack` and `../react-vite-swr/src/lib.rs` are this example's schema and
server — see that example's own README for their layout.

## Run the Dart/Flutter checks

```bash
cd examples/flutter-riverpod/client
flutter pub get
dart run build_runner build --delete-conflicting-outputs
flutter analyze --fatal-warnings --no-fatal-infos
flutter test

cd ../app
flutter pub get
flutter analyze --fatal-warnings --no-fatal-infos
flutter test
```

CI (`.github/workflows/ci.yml`'s `flutter-riverpod-example` job) runs exactly this sequence (via
`generate-dart --run-build-runner` rather than a separate `dart run build_runner build` line, so the
job doubles as coverage for that flag) — see the job's own comment for what it does and does not cover
(no `flutter build`/`flutter run`: no committed platform scaffolding, and no device/simulator target in
CI).

## Scope / known gaps

- The riverpod preset doesn't support `@@paged` models yet, and `list()`'s server-side filtering
  (`where`/structured filters) isn't wired into generated functions yet — same gaps `react-vite-swr`'s
  README documents for the `swr` preset; this schema and app avoid/work around them the same way.
- No login screen — a static `x-auth-id: 1` header (`app/lib/src/runtime.dart`) stands in for real
  auth, matching every other example in this repo.
- `TaskUpdateController`/`TaskDeleteController` are single global controllers (their `save`/`delete`
  methods take the target `id` as an argument, rather than the provider being keyed by id) — see
  `model_providers.dart.j2`'s own header comment. Fine at this app's scale.

## Real bugs found and fixed while building this (see the PR description for the full writeup)

1. **`CratestackDioAdapter` (REST plain-JSON adapter) never sent an `Accept` header.** Reproduced
   live: a request with no `Accept` header against a real server wired with only `JsonCodec` got "no
   encoder configured for response Content-Type application/cbor" back, not JSON. Fixed in
   `crates/cratestack-client-dart/templates/rest-runtime.dart.j2` — now sends `Accept: application/json`
   explicitly, matching `CratestackCborDioAdapter`'s own explicit `Accept` and the TypeScript REST
   runtime's identical `headers.set("Accept", "application/json")`. This affects **both** presets
   (`rest-runtime.dart.j2` is shared), not just riverpod.
2. **`TaskUpdateController`/`TaskDeleteController` auto-disposed mid-flight.** These are `@riverpod`
   `AsyncNotifier` controllers; nothing in `board_detail_screen.dart` was watching them, so Riverpod's
   auto-dispose could tear one down *during* its own network await, and the generated controller's
   post-await `state = ...` threw "Cannot use the Ref of ... after it has been disposed." (reproduced
   live tapping a checkbox). This is an app-code bug, not a generator bug — fixed by `ref.watch`-ing
   both controllers at the screen level, the same pattern the app already used correctly for
   `taskCreateControllerProvider`'s loading state.
3. **Generated data classes have no `operator ==`/`hashCode`, breaking family-provider caching for
   non-primitive arguments.** `estimateFocusMinutesProvider(EstimateFocusMinutesArgs(...))` never
   settled — a fresh `EstimateFocusMinutesArgs` built on every rebuild is never `==` to the previous
   one, so every rebuild started a brand-new `loading` provider. This is a real generator gap, broad
   enough (shared `build_data_class`, both presets, a real deep-vs-shallow-equality design decision for
   relation fields) that it's filed as a follow-up rather than fixed inline —
   [cratestack#325](https://github.com/cratestack/cratestack/issues/325). Worked around in
   `board_detail_screen.dart` by memoizing the family provider's argument object per distinct
   `openCount` (ordinary Riverpod practice regardless of this bug).
