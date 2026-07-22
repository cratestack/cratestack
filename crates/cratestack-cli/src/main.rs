mod cli_handlers;
mod cli_support;
mod cli_types;
mod drift;
mod migrate;
mod schema_diff;

use anyhow::Result;
use clap::Parser;

use crate::cli_handlers::run;
use crate::cli_types::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;

    use crate::Cli;
    use crate::cli_support::{json_check_failure, json_check_success};
    use crate::cli_types::{Command, StudioCmd};

    #[test]
    fn json_success_payload_has_empty_diagnostics() {
        let payload = json_check_success(Path::new("schema.cstack"));
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["schema"], "schema.cstack");
        assert_eq!(payload["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn json_failure_payload_exposes_structured_diagnostic_fields() {
        let error = cratestack_parser::parse_schema("model User {\n  email String\n}\n")
            .expect_err("schema should fail validation");
        let payload = json_check_failure(Path::new("schema.cstack"), &error);
        let diagnostic = &payload["diagnostics"][0];

        assert_eq!(payload["ok"], false);
        assert_eq!(diagnostic["line"], 1);
        assert!(diagnostic["start"].as_u64().is_some());
        assert!(diagnostic["end"].as_u64().is_some());
        assert!(
            diagnostic["message"]
                .as_str()
                .expect("message should be a string")
                .contains("missing an @id field")
        );
    }

    #[test]
    fn generate_typescript_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
        ]);

        match cli.command {
            Command::GenerateTypeScript {
                schema,
                out,
                package_name,
                base_path,
                template_dir,
                check,
                full_selection,
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(package_name, "cratestack-client");
                assert_eq!(base_path, "/api");
                assert_eq!(template_dir, None);
                assert!(!check);
                assert!(!full_selection);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn generate_typescript_clap_accepts_full_selection_flag() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-typescript",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--full-selection",
        ]);

        match cli.command {
            Command::GenerateTypeScript { full_selection, .. } => {
                assert!(full_selection);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn studio_run_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "studio", "run"]);
        match cli.command {
            Command::Studio {
                cmd: StudioCmd::Run { config, bind },
            } => {
                assert_eq!(config, PathBuf::from("studio.toml"));
                assert!(bind.is_none());
            }
            _ => panic!("expected studio run command"),
        }
    }

    #[test]
    fn studio_init_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "studio", "init"]);
        match cli.command {
            Command::Studio {
                cmd: StudioCmd::Init { out, force },
            } => {
                assert_eq!(out, PathBuf::from("."));
                assert!(!force);
            }
            _ => panic!("expected studio init command"),
        }
    }

    #[test]
    fn diff_clap_defaults() {
        let cli = Cli::parse_from(["cratestack", "diff", "old.cstack", "new.cstack"]);
        match cli.command {
            Command::Diff { old, new, json } => {
                assert_eq!(old, PathBuf::from("old.cstack"));
                assert_eq!(new, PathBuf::from("new.cstack"));
                assert!(!json);
            }
            _ => panic!("expected diff command"),
        }
    }

    #[test]
    fn studio_eject_requires_out() {
        let result = Cli::try_parse_from(["cratestack", "studio", "eject"]);
        assert!(
            result.is_err(),
            "studio eject must require --out, got {:?}",
            result.map(|cli| cli.command)
        );
    }
}
