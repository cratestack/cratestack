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
    /// Issue #344: `--swr`'s per-model file name
    /// (`src/swr/models/{{ file_stem }}.ts`) is derived from
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
        "--swr: models `{first}` and `{second}` both normalize to the file name \
         `src/swr/models/{file_stem}.ts` — rename one of them so their kebab-case forms differ"
    )]
    SwrModelFileNameCollision {
        first: String,
        second: String,
        file_stem: String,
    },
    /// The schema declares a composite primary key (`@@id([...])`) on at
    /// least one model. `include_*_schema!` has rejected these since the
    /// gap was found (see `cratestack_core::composite_id`), but this
    /// generator had no equivalent guard and instead panicked inside
    /// `views.rs`'s `primary_key_field(model).expect(...)` — a panic
    /// rather than an error, carrying a message (`validated schemas
    /// always have an id field`) that is simply false: the parser accepts
    /// such a schema. Same rejection, same wording, as the macro path.
    #[error("{0}")]
    CompositePrimaryKeyUnsupported(String),
}
