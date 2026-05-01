# CrateStack

CrateStack is a Rust-native, schema-first framework workspace for building typed HTTP APIs, generated clients, and backend services from `.cstack` files.

The target shape is intentionally broader than the currently implemented bootstrap slice:

* `cratestack` owns schema parsing, generated CRUD, generated procedures, generated HTTP routes, and generated clients
* generated HTTP routes are the canonical service APIs, including internal service-to-service usage
* CRUD exposure is schema-configurable per model, with ZenStack-like control over what is exposed
* non-CRUD workflows stay schema-declared as procedures and are implemented through generated Rust traits
* request authentication stays outside the framework core; applications implement `AuthProvider` so generated handlers resolve auth through one host-owned trait boundary
* JSON and CBOR are first-class codecs, while COSE is treated as an optional envelope layer over encoded bytes
* transport architecture is split into codec, framing, and envelope layers; `application/cbor-seq` belongs to framing rather than being treated as just another peer codec
* Rust client generation is already available today through compile-time `include_schema!` codegen for full schema consumers and `include_client_macro!` for client-only consumers, while standalone Rust-client CLI generation remains a follow-up; Dart client generation targets Riverpod-oriented frontends
* field visibility and filterability are controlled by separate schema directives
* custom fields are schema-declared and resolved through generated resolver traits

The currently implemented slice in this repo provides:

* a Rust 2024 multi-crate workspace under `cratestack/`
* core shared types for schema metadata, auth context, errors, codecs, and envelopes
* a `cratestack-policy` crate that now owns the canonical policy literals, predicates, expressions, and procedure-policy evaluation surface shared across the workspace
* a parser and semantic checker for an initial `.cstack` subset
* a compile-time `include_schema!` macro that validates schemas and emits a generated `cratestack_schema` module with summary metadata, model structs, generated procedure traits, schema-specific SQLx delegate scaffolding, generated Rust projection builders/wrappers, and generated Rust client facades
* a compile-time `include_client_macro!` macro that validates the same schema but emits only client-facing Rust types, inputs, projection builders, procedure payloads, and the generated Rust client facade, omitting SQLx delegates, generated Axum routers, policy authorizers, procedure registries, custom-field resolver traits, and event subscriptions
* a first-party CBOR codec crate
* a CLI with `cratestack check`, `cratestack print-ir`, `cratestack generate-dart`, and `cratestack generate-studio`
* a `cratestack-sqlx` crate with PostgreSQL runtime primitives plus generated delegate scaffolding for `create`, `find_many`, `find_unique`, `update`, and `delete`
* generated model and procedure policy enforcement for the current supported policy subset, with canonical policy types now shared through `cratestack-policy`
* a `cratestack-axum` crate with generated procedure routes plus generated model CRUD routes
* generated custom-field resolver trait metadata for schema fields marked with `@custom`
* generated list-route query parsing for canonical `fields`, nested declared-relation `include` paths, relation-specific `includeFields[path]`, canonical `sort`, `limit`, `offset`, scalar equality filters, operator suffixes such as `__ne`, `__lt`, `__lte`, `__gt`, `__gte`, `__in`, `__contains`, `__startsWith`, and `__isNull`, plus `cuid`, UUID, DateTime, canonical grouped `where=` expressions, legacy top-level `or=` groups, and recursive relation filters driven by explicit `@relation(fields:[...],references:[...])` metadata; grouped `where=` uses `not(...)` > `,` (AND) > `|` (OR), all left-associative within a precedence level, while relation-aware ordering stays limited to nested to-one paths and any to-many `sort` path remains unsupported; legacy `orderBy` remains accepted as a compatibility alias for `sort`
* generated model query-contract metadata for allowed `fields`, `include`, and `sort` selections, with route-level validation errors for unknown or disallowed selections
* generated typed relation filter/order DSL support in Rust delegates, including quantified to-many filters (`some`, `every`, `none`) and builder-style `FilterExpr` composition with `.and(...)`, `.or(...)`, and `.not()`
* generated nested projection builders in Rust via `cratestack_schema::{model}::select()` and `include_selection()`, with typed selected-payload wrappers for root resources and nested included relations
* a generated schema-native Rust client facade over `cratestack-client-rust`, covering typed model CRUD, procedures, and selected `get/list` helpers while still serializing the canonical HTTP projection contract
* generated client slices for Rust, Dart, and Flutter bridge/runtime experiments, with generated Dart now emitting a Flutter-shaped package, exposing canonical query params such as `fields`, `include`, `includeFields[path]`, `sort`, `limit`, `offset`, `where`, and legacy `or`, plus generated selection builders and projection-driven `getView` / `listView` helpers
* built-in `Page<T>` procedure return support for declared model/type items, with a canonical envelope shaped as `items`, `totalCount`, and `pageInfo`
* opt-in `@@paged` support for generated model list routes, so selected models can return `Page<Model>` from `GET /{models}` while the default list contract remains a plain array
* generated `tracing` instrumentation for procedure wrappers, generated procedure routes, and generated model list routes, while keeping subscriber/exporter setup host-owned
* request-authorizer hooks in `cratestack-client-rust` built around canonical request strings and encoded request body bytes, so host integrations can attach signed-request headers without changing generated client APIs
* a first-class `RequestContext` + `AuthProvider` integration surface for generated axum routers, plus `Cratestack::bind_auth(...)` / `bind_context(...)` for bound authenticated delegate usage outside HTTP
* a structured principal model inside `CoolContext` built around `principal.actor`, `principal.session`, `principal.tenant`, and free-form claims, while preserving legacy `auth().field` compatibility for existing schemas

The source of truth today is `README.md` plus the standalone docs project under `../cratestack-docs/`, with site navigation configured in `../cratestack-docs/mint.json`.

Studio-specific current-state note:

* `cratestack generate-studio` now supports one or more `.cstack` files in a single Yew + Rust Studio app by repeating `--schema` and `--service-url`
* the verified implementation snapshot lives in `../cratestack-docs/docs/studio/current-state.md`
* manifest-driven Studio generation is not implemented yet

Repo-specific adoption guide:

* `../cratestack-docs/docs/adoption/vaam-catalog.md` documents the first end-to-end path for using CrateStack with `vaam-backends/services/catalog-service` and `frontends/vaam-mobile`, including:
  * the shared `catalog.cstack` schema location
  * the generated Dart package path under `frontends/vaam-mobile/packages/gen_*`
  * the mobile Rust runtime path under `frontends/vaam-mobile/rust/vaam_runtime`, which now owns generic request execution/signing helpers rather than schema-typed catalog APIs
* `../cratestack-docs/docs/guides/auth-provider.md` documents the current host-auth boundary around `AuthProvider`, `RequestContext`, and `bind_auth(...)`
* `../cratestack-docs/docs/guides/telemetry.md` documents the current generated `tracing` coverage, emitted fields, and host-owned subscriber boundary
* `../cratestack-docs/docs/architecture/transport-architecture.md` is the canonical transport design reference for codec, framing, and envelope boundaries
* `../cratestack-docs/docs/architecture/http-transport-contract.md` is the canonical HTTP-wire contract for request and response negotiation behavior

The implementation is still narrower than the target-state ADR and PRD. COSE runtime integration, production-ready non-Rust selection typing, ZenStack-like exposure controls, runtime custom-field resolution, and negotiated multi-transport routing remain in progress.

Transport-specific current-state note:

* generated Axum routes currently enforce a single configured codec per router rather than negotiated multi-codec transport
* `application/cbor-seq` is a documented target transport mode, but it is not implemented yet
* COSE remains a planned envelope seam and is not implemented yet

Important current-state note:

* Rust client generation exists today through compile-time `include_schema!` or client-only `include_client_macro!` codegen rather than a standalone `generate-rust` CLI command
* Dart package generation is already automated through `cratestack generate-dart`
* the generated Dart package still depends on an app-provided `CratestackRuntimeBridge`; the direct exported Rust ABI path is still deferred
* auth-aware routing no longer requires per-service generated-route context resolver glue; host services can provide a single `AuthProvider` implementation and reuse `bind_auth(...)` for internal Rust callers
* policy types now live canonically in `cratestack-policy`, and both model/procedure policy lowering reuse that shared surface instead of defining parallel runtime enums in multiple crates

## VS Code

CrateStack has two different editor integration surfaces:

* Rust files that consume `cratestack::include_schema!(...)`
* `.cstack` schema files themselves

### Rust autocomplete and linting today

Rust-side editor support is project-dependent because `include_schema!` expands relative to a real Cargo project and a real schema path.

For VS Code, use `rust-analyzer` and point it at the Cargo workspace or workspaces that actually compile the schema consumer.

Recommended workspace settings:

```json
{
  "rust-analyzer.linkedProjects": [
    "cratestack/Cargo.toml",
    "vaam-backends/Cargo.toml"
  ],
  "rust-analyzer.procMacro.enable": true,
  "rust-analyzer.cargo.buildScripts.enable": true,
  "rust-analyzer.checkOnSave": true,
  "rust-analyzer.check.allTargets": true
}
```

Why this is needed:

* this repo root is not a single Cargo workspace
* generated Rust APIs come from proc-macro expansion, so `rust-analyzer.procMacro.enable` must stay on
* the editor only knows the generated `cratestack_schema` surface when it can build the real Cargo context that owns the schema

Current recommendation:

* document project-local VS Code settings in the consuming repo
* treat `cratestack/README.md` as the canonical explanation of why the Rust setup is project-dependent

### `.cstack` linting and autocomplete today

`.cstack` editor support should not depend on the full host project being checked out.

Current repo reality:

* `cratestack-cli` supports `check`, `check --format json`, `print-ir`, and `generate-dart`
* `crates/cratestack-lsp` now provides a standalone `.cstack` language server binary with diagnostics, hover, go-to-definition, document symbols, and basic completion, including relation-aware definition jumps from `@relation(fields:[...],references:[...])`
* `packages/cratestack-vscode` now provides the thin VS Code extension wrapper that launches `cratestack-lsp`, preferring a bundled server binary when present
* `packages/cratestack-vscode` also contributes basic `.cstack` syntax highlighting through a bundled TextMate grammar
* the parser still validates an initial schema subset rather than the full target grammar in the docs

Minimal local workflow:

1. build the language server from `cratestack/` with `cargo build -p cratestack-lsp`
2. install or run `packages/cratestack-vscode`
3. point the VS Code extension at the built binary through `cratestack.lsp.path` if it is not already on `PATH` and not already bundled in the extension package
4. use `cratestack check --format json --schema path/to/schema.cstack` for CI or editor-fallback diagnostics

Canonical editor-tooling documentation lives in `../cratestack-docs/docs/tooling/editor-tooling.md`.

Near-term follow-up work is:

1. add code actions for common relation mistakes
2. add semantic tokens on top of the TextMate grammar
3. add rename and references support using the newer precise spans
4. broaden extension-host end-to-end tests
5. trim VSIX contents and add missing package metadata

Recommended packaging boundary:

* `cratestack-parser`: parse source into a syntax tree plus schema IR with source spans
* `cratestack-core`: stable shared syntax and semantic data types that can be reused by macro, CLI, and LSP
* `cratestack-cli`: human-facing commands plus machine-readable diagnostics for CI and editor fallback use
* `cratestack-lsp`: editor protocol server for linting, hover, completion, go-to-definition, rename, and formatting later if desired
* `cratestack-vscode`: the extension wrapper that contributes the `.cstack` language and launches the server

This split keeps local project builds optional for basic schema editing while still allowing richer workspace-aware features later.

### Generated comments and Rust docs

Generated Rust docs should come from schema-authored documentation, not from hand-maintained Rust wrappers.

Current repo reality:

* leading `///` comments now attach to schema declarations and fields during parsing
* procedure docs can now carry `/// @param name ...` tags that attach documentation to generated procedure arguments
* the schema data model now carries documentation fields on declarations and fields, and stores spans on procedure arguments for editor use
* the proc-macro now emits Rust `#[doc = "..."]` attributes for generated structs, generated inputs, generated fields, and procedure modules

This gives one documentation source for:

* schema authors reading `.cstack`
* Rust users consuming generated APIs
* future hover text inside the `.cstack` language server

For the full current-state writeup and future roadmap, see `../cratestack-docs/docs/tooling/editor-tooling.md`.

## How It Fits Together

```mermaid
flowchart LR
    Schema[.cstack schema] --> Macro[include_schema!]
    Macro --> RustAPI[Generated Rust delegates]
    Macro --> HttpAPI[Generated Axum routes]
    Macro --> ClientMeta[Generated client metadata]
    RustAPI --> SQLx[SQLx runtime]
    HttpAPI --> Codec[Configured codec\nCBOR today]
    HttpAPI --> Auth[Host-provided auth context]
    ClientMeta --> Dart[Dart templates]
    ClientMeta --> RustClient[Rust client runtime]
```

## Common Use Cases

CrateStack is currently strongest for:

* internal CRUD-heavy services that want schema-defined policy checks
* CBOR-first HTTP APIs where JSON should not be assumed everywhere
* teams that want generated REST routes and a typed Rust delegate API from one schema
* services that need relation-aware filtering and sorting without hand-writing every query parser
* Rust callers that want nested projection helpers without hand-writing `fields` and `includeFields[...]` strings
* teams that want generated CRUD/procedure clients on top of the same schema that drives routes and delegates

It is not yet the right fit for:

* highly customized non-REST transport protocols
* production-stable exact typed non-Rust client generation across arbitrary projection shapes

## Generated Dart Package

The current Dart generator emits multiple sibling files rather than one monolithic file.

Example output for a generated `blog_client/` package directory:

* `pubspec.yaml`
* `README.md`
* `CHANGELOG.md`
* `analysis_options.yaml`
* `lib/blog_client.dart`
* `lib/src/runtime.dart`
* `lib/src/queries.dart`
* `lib/src/constants.dart`
* `lib/src/models.dart`
* `lib/src/apis.dart`
* `example/main.dart`
* `test/blog_client_test.dart`

The main file re-exports the siblings, so callers can still import a single entrypoint.

Because the generated client currently includes Riverpod providers, the emitted package is shaped as a Flutter package rather than a Dart-only package.

The typed Dart query helpers now expose the canonical HTTP query params documented in this README.

Example generated query usage:

```dart
final postListSelection = PostSelection()
  ..id()
  ..title()
  ..author((author) => author.email());

final posts = await client.posts.list(
  query: postListSelection.toListQuery(
    sort: '-id',
    limit: 20,
    offset: 0,
    where: 'published=true',
  ),
);

final post = await client.posts.get(
  1,
  query: postListSelection.toFetchQuery(),
);

final projectedPostSelection = PostSelection()
  ..id()
  ..author((author) {
    author.email();
    author.profile((profile) => profile.nickname());
  });

final projectedPost = await client.posts.getView(
  1,
  projection: projectedPostSelection.asProjection(),
);

final feed = await client.procedures.getFeed(
  const GetFeedArgs(limit: 10),
);

final publishedPost = await client.procedures.publishPost(
  const PublishPostArgs(
    args: PublishPostInput(postId: 1),
  ),
);
```

For plain `list(...)` or `get(...)` calls, prefer `selection.toListQuery(...)` and `selection.toFetchQuery()` when you want projection-shaped query ergonomics without switching return types. Use `selection.asProjection()` with `getView(...)` or `listView(...)` when you want projected wrapper types back.

Why this helps:

```dart
// Before
final posts = await client.posts.list(
  query: const CratestackListQuery(
    fields: [PostFieldNames.id, PostFieldNames.title],
    include: [PostIncludeNames.author],
    includeFields: {
      PostIncludeNames.author: [UserFieldNames.email],
    },
    sort: '-id',
    limit: 20,
    where: 'published=true',
  ),
);

// After
final selection = PostSelection()
  ..id()
  ..title()
  ..author((author) => author.email());

final posts = await client.posts.list(
  query: selection.toListQuery(
    sort: '-id',
    limit: 20,
    where: 'published=true',
  ),
);
```

The builder path removes repetitive field-path plumbing, keeps nested relation intent close to the data shape, and makes it easier to evolve a screen query without juggling parallel `fields`, `include`, and `includeFields[path]` arrays by hand.

The generated field/include constants still matter. They remain the low-level escape hatch when app code needs to assemble query parts dynamically, persist a compact query shape, or expose user-driven field selection without hard-coding a generated builder flow.

The current typed client still uses generic value-graph conversion internally for model shaping, but it now generates selection builders plus projection wrappers for projected reads. Exact type-level projection remains intentionally looser on Dart than on Rust.

Flutter or Riverpod selection example:

```dart
final blogPostListProvider = FutureProvider((ref) async {
  final client = ref.watch(blogClientClientProvider);
  final selection = PostSelection()
    ..id()
    ..title()
    ..author((author) => author.email());

  return client.posts.list(
    query: selection.toListQuery(
      sort: '-id',
      limit: 20,
      where: 'published=true',
    ),
  );
});
```

Flutter or Riverpod projection example:

```dart
final blogPostCardProvider = FutureProvider.family((ref, int id) async {
  final client = ref.watch(blogClientClientProvider);
  final selection = PostSelection()
    ..id()
    ..title()
    ..author((author) => author.email());

  return client.posts.getView(
    id,
    projection: selection.asProjection(),
  );
});
```

Flutter or Riverpod `@@paged` full-model example:

```dart
final pagedPostsProvider = FutureProvider((ref) async {
  final client = ref.watch(blogClientClientProvider);
  final selection = PostSelection()
    ..id()
    ..title()
    ..author((author) => author.email());

  return client.posts.list(
    query: selection.toListQuery(
      sort: '-id',
      limit: 20,
      offset: 0,
      where: 'published=true',
    ),
  );
});

Widget buildPagedPosts(WidgetRef ref) {
  final page = ref.watch(pagedPostsProvider);

  return page.when(
    data: (page) => ListView.builder(
      itemCount: page.items.length,
      itemBuilder: (context, index) {
        final post = page.items[index];
        return ListTile(title: Text(post.title));
      },
    ),
    loading: () => const CircularProgressIndicator(),
    error: (error, _) => Text('$error'),
  );
}
```

Flutter or Riverpod `@@paged` projection example:

```dart
final pagedPostCardsProvider = FutureProvider((ref) async {
  final client = ref.watch(blogClientClientProvider);
  final selection = PostSelection()
    ..id()
    ..title()
    ..author((author) => author.email());

  return client.posts.listView(
    projection: selection.asProjection(),
    query: const CratestackListQuery(
      limit: 20,
      offset: 0,
      sort: '-id',
      where: 'published=true',
    ),
  );
});

String describeProjectedPage(Page<ProjectedPost> page) {
  return 'items=${page.items.length} total=${page.totalCount} hasNext=${page.pageInfo.hasNextPage}';
}
```

Flutter UI example:

```dart
class BlogPostListScreen extends ConsumerWidget {
  const BlogPostListScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final page = ref.watch(pagedPostsProvider);

    return Scaffold(
      appBar: AppBar(title: const Text('Posts')),
      body: page.when(
        data: (page) => Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text('Total: ${page.totalCount ?? page.items.length}'),
            ),
            Expanded(
              child: ListView.builder(
                itemCount: page.items.length,
                itemBuilder: (context, index) {
                  final post = page.items[index];
                  return ListTile(
                    title: Text(post.title),
                    subtitle: Text('hasNextPage=${page.pageInfo.hasNextPage}'),
                  );
                },
              ),
            ),
          ],
        ),
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (error, _) => Center(child: Text('$error')),
      ),
    );
  }
}

class BlogPostCard extends ConsumerWidget {
  const BlogPostCard({super.key, required this.id});

  final int id;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final post = ref.watch(blogPostCardProvider(id));

    return post.when(
      data: (post) => Card(
        child: ListTile(
          title: Text(post.title ?? ''),
          subtitle: Text(post.author?.email ?? ''),
        ),
      ),
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, _) => Center(child: Text('$error')),
    );
  }
}
```

For `@@paged` models:

- `client.posts.list(...)` returns `Future<Page<Post>>`
- `client.posts.listView(...)` returns `Future<Page<ProjectedPost>>`
- the stable paging envelope is `items`, `totalCount`, and `pageInfo`
- only `items` changes shape between full-model and projection flows

## Schema Enums

Short version: enums work for generated Rust and Dart clients now. 🎉

CrateStack schemas now support top-level `enum` declarations:

```cool
enum Role {
  admin
  member
}
```

What works today:

- generated Rust schema code emits real Rust enums
- generated Dart packages emit real Dart enums
- enum-typed model fields, type fields, procedure args, and procedure returns round-trip through generated Rust and Dart clients

Example schema:

```cool
enum PaymentInstrumentStatus {
  active
  inactive
}

model PaymentInstrument {
  id String @id
  status PaymentInstrumentStatus
}
```

Example generated Dart shape:

```dart
enum PaymentInstrumentStatus {
  active('active'),
  inactive('inactive');

  const PaymentInstrumentStatus(this.wireName);

  final String wireName;

  static PaymentInstrumentStatus fromWire(Object? value) {
    switch (value as String) {
      case 'active':
        return PaymentInstrumentStatus.active;
      case 'inactive':
        return PaymentInstrumentStatus.inactive;
    }
    throw ArgumentError.value(value, 'value');
  }

  Object toWire() => wireName;
}
```

Current boundary:

- read-policy literal lowering still treats only required `Boolean`, `Int`, and `String` fields as literal-comparable at macro expansion time
- if you want policy expressions like `auth().role == "admin"` on an enum-typed field, the next implementation step is to teach macro lowering to treat required enum fields as string-backed policy literals

Translation: data models are happy, policy literals are still a little old-fashioned. 😄

Works today:

```cool
enum DeviceKeyStatus {
  active
}

model DeviceKey {
  keyId String @id
  status DeviceKeyStatus
}
```

Not supported yet in read-policy literal lowering:

```cool
enum Role {
  admin
  user
}

auth SessionUser {
  role Role
}

model User {
  id Int @id

  @@allow("read", auth().role == "admin")
}
```

## Using In `vaam-mobile`

Today there are two distinct integration paths for `frontends/vaam-mobile`.

### 1. What works today

Generate the Flutter-shaped client package into the mobile workspace, add it as a path dependency, and provide a `CratestackRuntimeBridge` implementation from the app side.

Example generation command from `cratestack/`:

```bash
cargo run -p cratestack-cli -- generate-dart \
  --schema "crates/cratestack/tests/fixtures/blog.cstack" \
  --out "../frontends/vaam-mobile/packages/blog_client" \
  --library-name blog_client \
  --base-path "/api"
```

Run the same command again whenever either of these changes:

- the source `.cstack` schema
- the Dart generator or its templates under `cratestack/crates/cratestack-client-dart/`

That regeneration step is how schema enum additions reach packages such as `frontends/vaam-mobile/packages/gen_auth_client`.

If you skip regeneration, the generated package will politely keep living in the past. ⏰

Example auth client regeneration from `cratestack/`:

```bash
cargo run -p cratestack-cli -- generate-dart \
  --schema "../vaam-backends/services/auth-service/schema/auth.cstack" \
  --out "../frontends/vaam-mobile/packages/gen_auth_client" \
  --library-name gen_auth_client \
  --base-path "/api"
```

Then add it to `frontends/vaam-mobile/pubspec.yaml`:

```yaml
dependencies:
  blog_client:
    path: packages/blog_client
```

And provide the generated runtime bridge provider in app code:

```dart
ProviderScope(
  overrides: [
    blogClientRuntimeBridgeProvider.overrideWith((ref) => myBridge),
    blogClientBasePathProvider.overrideWith((ref) => '/api'),
  ],
  child: const App(),
)
```

This path is usable now because the generated package already exposes:

* typed CRUD APIs
* typed procedure APIs
* Riverpod providers
* query helpers and generated constants

### 2. Intended long-term path

The intended end state is still Rust-owned runtime behavior behind a Dart-accessible bridge layer.

That future path keeps:

* CBOR / JSON transport codec choice in Rust
* future COSE envelope behavior in Rust
* future signing/canonicalization in Rust

The generated Dart package surface is meant to stay stable while the bridge implementation matures.

### What is still missing for the full Rust path

The following client-side pieces are not yet complete end-to-end:

* a raw exported ABI wrapper for direct Dart FFI consumers
* a public Flutter or Dart-facing persisted-state API
* runtime-configurable SQLite selection from the Flutter-facing wrapper
* a first-class typed remote-error surface in the generated Dart APIs
* fully selection-aware response typing when `fields`, `include`, and `includeFields[path]` narrow payloads

So for `vaam-mobile`, the current recommendation is:

1. generate the package into `frontends/vaam-mobile/packages/...`
2. use the generated package API and Riverpod providers now
3. treat the bridge implementation as the swap point for future Rust runtime improvements

## Quick Start

Example schema:

```cool
model User {
  id Int @id
  email String @unique
  role String
}

model Post {
  id Int @id
  title String
  subtitle String?
  published Boolean
  authorId Int
  author User @relation(fields:[authorId],references:[id])

  @@allow("read", published || authorId == auth().id)
}
```

Rust setup:

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");

let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
let cool = cratestack_schema::Cratestack::builder(pool)
    .codec(cratestack_codec_cbor::CborCodec::default())
    .build();

let app = axum::Router::new().nest(
    "/api",
    cratestack_schema::axum::model_router(
        cool,
        cratestack_codec_cbor::CborCodec,
        resolve_context,
    ),
);
```

Delegate usage:

```rust
let visible_posts = cool
    .post()
    .find_many()
    .where_expr(
        cratestack_schema::post::author()
            .email()
            .eq("owner@example.com")
            .and(cratestack_schema::post::published().is_true()),
    )
    .order_by(cratestack_schema::post::author().email().desc())
    .limit(20)
    .run(&ctx)
    .await?;
```

Rust projected-fetch usage:

```rust
let selection = cratestack_schema::post::select()
    .id()
    .title()
    .include_author_selected(
        cratestack_schema::user::include_selection().email(),
    );

let projected_post = client
    .get_view(
        "/posts/1",
        &selection,
        &[],
    )
    .await?;

let title = projected_post.title()?;
let author_email = projected_post.author()?.and_then(|author| author.email().ok());
```

Client-only Rust setup for backend-to-backend callers:

```rust
use cratestack::include_client_macro;
use cratestack::client_rust::{CborCodec, ClientConfig, CratestackClient};

include_client_macro!("../payment-gateway/schema/payment.cstack");

let base_url = url::Url::parse(&std::env::var("PAYMENT_GATEWAY_URL")?)?;
let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
let payment = cratestack_schema::client::Client::new(runtime);

let providers = payment
    .procedures()
    .supported_payment_providers(
        &cratestack_schema::procedures::supported_payment_providers::Args::default(),
        &[("authorization", authorization_header.as_str())],
    )
    .await?;
```

Use `include_client_macro!` when the crate only needs to call another CrateStack-generated HTTP API. For backend-to-backend calls, construct the runtime with `CborCodec` by default; keep JSON for debugging, tests, and compatibility exceptions. Use `include_schema!` when the crate owns that schema's database/runtime surface and needs generated SQLx delegates, generated Axum routers, procedure registries, policy helpers, custom-field resolver traits, or event subscriptions. OAuth2 protocol endpoints stay outside `.cstack` and should remain handwritten protocol integrations rather than generated CrateStack clients.

## HTTP Examples

Canonical list query examples:

```text
GET /api/posts?fields=id,title&sort=-author.email,id&limit=20&offset=0
GET /api/posts?include=author&where=published=true
GET /api/posts?fields=id,title&include=author&includeFields[author]=email
GET /api/users?where=not(sessions.some.label__contains=Revoked)
GET /api/sessions?where=(label__startsWith=Primary|label__startsWith=Revoked),createdAt__gt=2026-01-01T12:00:00Z
```

Projection rules:

* `fields` selects scalar fields on the primary resource.
* `include` materializes directly declared relations.
* `includeFields[path]` narrows the scalar fields returned for that already-included relation path.
* `includeFields[path]` is rejected unless that exact relation path also appears in `include`.

Common projection use-cases:

* mobile list screens that only need `id`, `title`, and a small related badge or author label
* admin tables that need narrow top-level columns plus one related identifier or email
* feed/detail hybrids where the main record stays compact while one or more nested relation paths are embedded for display
* bandwidth-sensitive clients that want relation data without paying for the full related object shape

Mental model:

* use `fields` for "which columns from this resource?"
* use `include` for "which relation paths should be embedded?"
* use `includeFields[path]` for "which columns from that embedded relation path?"

Raw `curl` examples:

```bash
curl \
  -H 'Accept: application/cbor' \
  'http://127.0.0.1:3000/api/posts?fields=id,title&sort=-id&limit=10'

curl \
  -H 'Accept: application/cbor' \
  -H 'x-auth-id: 1' \
  'http://127.0.0.1:3000/api/posts?include=author&where=author.profile.nickname=Zulu'

curl \
  -H 'Accept: application/cbor' \
  -H 'x-auth-id: 1' \
  'http://127.0.0.1:3000/api/posts?fields=id,title&include=author&includeFields[author]=email'

curl \
  -H 'Accept: application/cbor' \
  -H 'x-auth-id: 1' \
  'http://127.0.0.1:3000/api/users?where=not(sessions.every.revokedAt__isNull=true)'
```

CRUD write example:

```bash
curl \
  -X POST \
  -H 'Content-Type: application/cbor' \
  -H 'Accept: application/cbor' \
  -H 'x-auth-id: 1' \
  --data-binary @create-post.cbor \
  'http://127.0.0.1:3000/api/posts'
```

## Query Mental Model

```mermaid
flowchart TD
    A[HTTP query] --> B{Canonical key?}
    B -->|sort/limit/offset| C[List controls]
    B -->|fields/include| D[Projection and nested relation materialization]
    B -->|where| E[Grouped boolean parser]
    B -->|resource filters| F[Scalar or relation filter builder]
    E --> G[FilterExpr tree]
    F --> G
    G --> H[SQLx query builder]
    C --> H
    D --> I[Route response shaping]
    H --> I
```

## Local DB Test Loop

`cratestack/compose.yml` provides a local PostgreSQL 18 instance for DB-backed tests.

```bash
docker compose -f cratestack/compose.yml up -d
export CRATESTACK_TEST_DATABASE_URL=postgres://cratestack:cratestack@127.0.0.1:55432/cratestack_test
cargo test --workspace
docker compose -f cratestack/compose.yml down
```

`cratestack/.env.test.example` records the expected `CRATESTACK_TEST_DATABASE_URL` value.
