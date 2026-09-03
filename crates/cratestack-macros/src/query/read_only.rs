//! The `READ ONLY` transaction a generated `query` runs inside, and the
//! error mapping for the one SQLSTATE it exists to produce
//! (cratestack#867, remediating cratestack#870's review finding 1).
//!
//! **Why the database enforces this and not us.** The review measured a
//! `query` body writing: `WITH ins AS (INSERT … RETURNING …) SELECT …` is
//! an ordinary `SELECT` to the driver, so nothing about the statement's
//! shape stops it. `@allow` gated the *call*, but the write bypassed
//! `@@audit` rows, the `@@emit` outbox, `@version` optimistic locking,
//! soft-delete, `@@internal` suppression and the target model's own write
//! `@@allow` — an escape hatch around every guarantee the framework's own
//! write path provides.
//!
//! Rejecting DML by inspecting the SQL text was never a real option: it
//! means classifying arbitrary SQL, which is the parsing design §3 prices
//! out and rejects, and a keyword blocklist is exactly the kind of check
//! that looks right and is bypassable. Postgres already holds the
//! authority — `SET TRANSACTION READ ONLY` refuses DML and DDL with
//! SQLSTATE `25006` from inside the engine, whatever the statement looks
//! like. Enforcement, not detection.

use quote::quote;

/// The `BEGIN; SET TRANSACTION READ ONLY; <query>; COMMIT` wrapper, as an
/// expression producing `Result<Output, CratestackError>`.
///
/// `fetch` is `fetch_one` or `fetch_all`; `element_type` is the row type
/// (`query_as` decodes one row at a time, so a `T[]` query still names
/// `T` here).
pub(super) fn read_only_execution(
    element_type: &proc_macro2::TokenStream,
    fetch: &proc_macro2::TokenStream,
    binds: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        async {
            let mut connection = db
                .pool()
                .acquire()
                .await
                .map_err(::cratestack::cratestack_error_from_sqlx)?;
            let mut transaction = connection
                .begin()
                .await
                .map_err(::cratestack::cratestack_error_from_sqlx)?;
            // Must be the first statement in the transaction: Postgres
            // refuses `SET TRANSACTION READ ONLY` once any query has run
            // inside it, so issuing it here — rather than alongside the
            // query — is what makes the guarantee hold instead of
            // silently no-opping.
            ::cratestack::sqlx::query("SET TRANSACTION READ ONLY")
                .execute(&mut *transaction)
                .await
                .map_err(::cratestack::cratestack_error_from_sqlx)?;

            let rows = ::cratestack::sqlx::query_as::<_, #element_type>(SQL)
                #(#binds)*
                .#fetch(&mut *transaction)
                .await
                .map_err(__query_error);

            // Committing a read-only transaction releases the snapshot;
            // rolling back would too, but commit keeps the "nothing
            // unusual happened" path indistinguishable from any other
            // successful read in the server's logs.
            let _ = transaction.commit().await;
            rows
        }
    }
}

/// The `__query_error` helper spliced into each query module.
pub(super) fn query_error_fn() -> proc_macro2::TokenStream {
    quote! {
        /// Maps a failure of the query statement itself.
        ///
        /// Everything reaches `cratestack_error_from_sqlx` unchanged
        /// except SQLSTATE `25006` — "cannot execute INSERT in a
        /// read-only transaction" — which becomes a fixed message naming
        /// the query and saying what to do instead.
        ///
        /// `Internal` rather than a 4xx on purpose. A `query` that writes
        /// is a *schema* bug, not a caller mistake — no argument the
        /// caller could have passed would have made it legal — and
        /// `CratestackError`'s 5xx variants carry operator-only detail
        /// that is never returned to a client, so the message can name
        /// the query without any risk of the schema's SQL reaching a
        /// response body.
        ///
        /// The driver's own error is logged here, before mapping,
        /// precisely because the mapping discards it: `run`'s
        /// `tracing::warn!` sees only the `CratestackError` and can report
        /// nothing but its code. Without this line an operator debugging a
        /// refused query would have the framework's explanation and no
        /// SQLSTATE — which is the half that says *which* statement
        /// Postgres objected to.
        fn __query_error(error: ::cratestack::sqlx::Error) -> ::cratestack::CratestackError {
            if let ::cratestack::sqlx::Error::Database(database_error) = &error {
                ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_query = NAME,
                    cratestack_operation = "query",
                    cratestack_sqlstate = database_error.code().as_deref().unwrap_or("unknown"),
                    cratestack_db_message = database_error.message(),
                    "cratestack query database error",
                );
                if database_error.code().as_deref() == Some("25006") {
                    return ::cratestack::CratestackError::Internal(format!(
                        "query `{}` attempted to modify data; a `query` block runs in a \
                         read-only transaction and may only read. Use a procedure or a model \
                         write builder instead.",
                        NAME,
                    ));
                }
            }
            ::cratestack::cratestack_error_from_sqlx(error)
        }
    }
}
