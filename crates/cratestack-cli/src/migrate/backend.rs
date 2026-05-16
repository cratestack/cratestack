//! Backend selection for `cratestack migrate` subcommands.

use crate::cli_types::MigrateBackendArg;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Backend {
    Postgres,
    Sqlite,
}

impl Backend {
    pub(super) fn slug(self) -> &'static str {
        match self {
            Backend::Postgres => "postgres",
            Backend::Sqlite => "sqlite",
        }
    }
}

pub(super) fn expand(arg: MigrateBackendArg) -> &'static [Backend] {
    match arg {
        MigrateBackendArg::Postgres => &[Backend::Postgres],
        MigrateBackendArg::Sqlite => &[Backend::Sqlite],
        MigrateBackendArg::Both => &[Backend::Postgres, Backend::Sqlite],
    }
}
