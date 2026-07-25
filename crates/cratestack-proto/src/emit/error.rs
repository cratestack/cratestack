//! Errors the `.proto` emitter (Part B/C of ticket #169) can raise. These
//! are all schema-shape problems the emitter itself catches — field-number
//! problems still surface as [`crate::PbLockError`] from `build_lock`.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProtoEmitError {
    /// A synthesized message name (`Create<M>Input`, `Update<M>Input`,
    /// `<Procedure>Input`, `<Procedure>Output`, `PageOf<Item>`) collides
    /// with a `model`/`type`/`enum` declared in the schema, or with another
    /// synthesized name. Deliberately a hard error rather than a rename:
    /// silently renaming would make the generated `.proto` diverge from
    /// what a reader of the schema would expect that name to mean.
    #[error(
        "synthesized message name `{name}` collides with {conflict}; rename the schema \
         declaration or the procedure/model that would synthesize `{name}`"
    )]
    MessageNameCollision { name: String, conflict: String },

    /// [`crate::PbLock::package`] was `None` when `.proto` text generation
    /// was requested. The CLI (ticket #169 Part D) is responsible for
    /// resolving and pinning `package` before calling [`super::emit_proto`];
    /// this is a defensive check for callers that skip that step.
    #[error(
        "cannot emit `.proto` text: the lock has no `package` set — resolve one \
         (see docs/design/protobuf.md §4.6) before calling emit_proto"
    )]
    MissingPackage,

    /// A field/message the emitter is about to render has no entry in the
    /// `PbLock` it was given. This should be unreachable in practice: the
    /// caller is expected to pass a lock freshly built (via `build_lock`)
    /// from the same `schema`/`extra_messages` pair being emitted. Kept as
    /// a checked error rather than a panic so a caller that violates that
    /// contract gets a message instead of an index-out-of-bounds.
    #[error(
        "no field-number lock entry for `{owner}.{field}`; was the lock built from this schema?"
    )]
    MissingLockEntry { owner: String, field: String },

    #[error("no field-number lock entry for message `{0}`; was the lock built from this schema?")]
    MissingMessageLock(String),

    #[error("no field-number lock entry for enum `{0}`; was the lock built from this schema?")]
    MissingEnumLock(String),
}
