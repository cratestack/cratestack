/// Split out of `templates.rs` (issue #304) to keep that file under this
/// repo's ~200-LoC convention as it grew a fan-out mechanism for the `swr`
/// preset — this enum is pure error data with no rendering logic, so it
/// moves cleanly on its own.
#[derive(Debug, thiserror::Error)]
pub enum TypeScriptGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
    /// `transport grpc` schema, but `TypeScriptGeneratorConfig::pb_lock`
    /// was `None`. The gRPC-Web wire codec needs the real field numbers
    /// `cratestack generate-proto` assigns — run that first (or pass its
    /// output through) before generating a `transport grpc` client.
    #[error(
        "schema declares `transport grpc`, which needs the schema's `.pb.lock` to generate a \
         gRPC-Web client — run `cratestack generate-proto` first and pass its lock via \
         `TypeScriptGeneratorConfig::pb_lock`"
    )]
    MissingPbLock,
    /// The lock parsed, but has no `package` — shouldn't happen for a lock
    /// `generate-proto` produced (`--package` is required on first run,
    /// `docs/design/protobuf.md` §4.6), but a hand-edited or pre-package
    /// lock is possible, so this is a real error rather than a panic.
    #[error(
        "the schema's `.pb.lock` has no `package` set — gRPC-Web method paths need it \
         (`/<package>.Api/<Method>`); re-run `cratestack generate-proto --package <name>`"
    )]
    MissingPbLockPackage,
    /// The lock is missing an entry (message or field) the schema expects
    /// — a stale lock relative to the schema. `cratestack generate-proto
    /// --check` is the tool that catches and reports this drift in detail;
    /// this generator just refuses to guess a field number. `field` is
    /// pre-formatted by the call site (` field \`x\`` or empty) rather than
    /// interpolated here, to keep the `#[error(...)]` format string a
    /// plain literal.
    #[error(
        "`.pb.lock` is missing an entry for message `{message}`{field}: re-run `cratestack generate-proto` to refresh it"
    )]
    MissingPbLockEntry { message: String, field: String },
    /// `--preset swr` combined with a `transport grpc` schema. The `swr`
    /// preset (issue #304, epic #298) targets RPC and REST only for its
    /// first pass — gRPC-Web is explicitly out of scope (see the epic's
    /// "Out" scope list) because its wire shape (typed protobuf fields, no
    /// URL-query shaping, no procedure surface — see `crate::grpc`'s module
    /// doc) doesn't fit the plain-fetch-function shape this preset emits.
    /// Use `--preset default` for a `transport grpc` schema today.
    #[error(
        "`--preset swr` does not support `transport grpc` schemas yet — gRPC-Web is out of \
         scope for issue #304; use `--preset default` for this schema"
    )]
    SwrPresetUnsupportedForGrpc,
    /// Issue #344: the `swr` preset's per-model file name
    /// (`src/models/{{ file_stem }}.ts`) is derived from
    /// `crate::naming::to_kebab_case`, which — like `to_camel_case`/
    /// `to_pascal_case`/`to_snake_case` — tokenizes through the same
    /// lossy `split_words` (splits on `_`/`-`/` ` *and* case boundaries).
    /// Two distinct, parser-valid model names (e.g. `UserGroup` and
    /// `User_Group`) can collapse to the same word sequence and therefore
    /// the same file path. Decision spike #317 ruled out a single
    /// parser-level check (each collision-prone call site normalizes
    /// differently, so no shared check can cover all of them); this call
    /// site fails loudly rather than disambiguating (contrast
    /// `crate::views::disambiguate_model_api_keys`, which suffixes a
    /// colliding *display* key) because a clobbered generated file is
    /// silent data loss a schema author has no way to notice short of
    /// diffing generator output on disk.
    #[error(
        "swr preset: models `{first}` and `{second}` both normalize to the file name \
         `src/models/{file_stem}.ts` — rename one of them so their kebab-case forms differ"
    )]
    SwrModelFileNameCollision {
        first: String,
        second: String,
        file_stem: String,
    },
}
