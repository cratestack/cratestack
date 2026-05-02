mod cli_handlers;
mod cli_support;
mod cli_types;

use anyhow::Result;
use clap::Parser;

use crate::cli_handlers::run;
use crate::cli_types::{Cli, Command, StudioProfileArg};

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser;

    use crate::cli_support::{
        ensure_output_dir_is_empty, json_check_failure, json_check_success, resolve_context_keys,
        slugify_path_token, validate_context_key, validate_mount_path, validate_service_url,
        validate_studio_context_inputs, validate_studio_name,
    };
    use crate::{Cli, Command, StudioProfileArg};

    #[test]
    fn json_success_payload_has_empty_diagnostics() {
        let payload = json_check_success(&Path::new("schema.cstack").to_path_buf());
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["schema"], "schema.cstack");
        assert_eq!(payload["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn json_failure_payload_exposes_structured_diagnostic_fields() {
        let error = cratestack_parser::parse_schema("model User {\n  email String\n}\n")
            .expect_err("schema should fail validation");
        let payload = json_check_failure(&Path::new("schema.cstack").to_path_buf(), &error);
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
    fn generate_studio_clap_defaults() {
        let cli = Cli::parse_from([
            "cratestack",
            "generate-studio",
            "--schema",
            "schema.cstack",
            "--out",
            "out",
            "--name",
            "inventory-studio",
            "--service-url",
            "http://127.0.0.1:8082",
        ]);

        match cli.command {
            Command::GenerateStudio {
                schema,
                service_url,
                context,
                mount_path,
                profile,
                ..
            } => {
                assert_eq!(schema, vec![PathBuf::from("schema.cstack")]);
                assert_eq!(service_url, vec!["http://127.0.0.1:8082".to_owned()]);
                assert!(context.is_empty());
                assert_eq!(mount_path, "/studio");
                assert_eq!(profile, StudioProfileArg::Dev);
            }
            _ => panic!("expected generate-studio command"),
        }
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
            } => {
                assert_eq!(schema, PathBuf::from("schema.cstack"));
                assert_eq!(out, PathBuf::from("out"));
                assert_eq!(package_name, "cratestack-client");
                assert_eq!(base_path, "/api");
                assert_eq!(template_dir, None);
            }
            _ => panic!("expected generate-typescript command"),
        }
    }

    #[test]
    fn validate_mount_path_rejects_missing_leading_slash() {
        let error = validate_mount_path("studio").expect_err("mount path should fail");
        assert!(error.to_string().contains("must begin with '/'"));
    }

    #[test]
    fn validate_mount_path_rejects_root_mount() {
        let error = validate_mount_path("/").expect_err("root mount path should fail");
        assert!(error.to_string().contains("not supported"));
    }

    #[test]
    fn validate_service_url_rejects_relative_url() {
        let error = validate_service_url("inventory-service:8082")
            .expect_err("relative service url should fail");
        assert!(error.to_string().contains("must be absolute"));
    }

    #[test]
    fn validate_service_url_rejects_missing_host() {
        let error = validate_service_url("file:///tmp/schema.cstack")
            .expect_err("service URL without host should fail");
        assert!(error.to_string().contains("must be absolute"));
    }

    #[test]
    fn validate_studio_context_inputs_rejects_mismatched_lengths() {
        let error = validate_studio_context_inputs(
            &[PathBuf::from("a.cstack"), PathBuf::from("b.cstack")],
            &["http://127.0.0.1:8081".to_owned()],
            &[],
        )
        .expect_err("mismatched inputs should fail");
        assert!(
            error
                .to_string()
                .contains("same number of --schema and --service-url")
        );
    }

    #[test]
    fn validate_context_key_rejects_spaces() {
        let error = validate_context_key("inventory admin")
            .expect_err("spaces should fail context key validation");
        assert!(error.to_string().contains("not URL-safe"));
    }

    #[test]
    fn resolve_context_keys_derives_unique_defaults() {
        let keys = resolve_context_keys(
            &[
                PathBuf::from("services/inventory-service/schema/inventory.cstack"),
                PathBuf::from("services/accounts-service/schema/accounts.cstack"),
            ],
            &[],
        )
        .expect("context keys should derive");

        assert_eq!(keys, vec!["inventory".to_owned(), "accounts".to_owned()]);
    }

    #[test]
    fn resolve_context_keys_accepts_explicit_values() {
        let keys = resolve_context_keys(
            &[PathBuf::from("a.cstack"), PathBuf::from("b.cstack")],
            &["inventory".to_owned(), "accounts_api".to_owned()],
        )
        .expect("explicit context keys should be accepted");
        assert_eq!(
            keys,
            vec!["inventory".to_owned(), "accounts_api".to_owned()]
        );
    }

    #[test]
    fn resolve_context_keys_rejects_duplicates() {
        let error = resolve_context_keys(
            &[PathBuf::from("a.cstack"), PathBuf::from("b.cstack")],
            &["dup".to_owned(), "dup".to_owned()],
        )
        .expect_err("duplicate context keys should fail");
        assert!(error.to_string().contains("is duplicated"));
    }

    #[test]
    fn resolve_context_keys_rejects_invalid_explicit_values() {
        let error = resolve_context_keys(&[PathBuf::from("a.cstack")], &["invalid key".to_owned()])
            .expect_err("invalid context key should fail");
        assert!(error.to_string().contains("not URL-safe"));
    }

    #[test]
    fn resolve_context_keys_prefixes_numeric_file_stem() {
        let keys = resolve_context_keys(&[PathBuf::from("01-admin.cstack")], &[])
            .expect("derived key should succeed");
        assert_eq!(keys, vec!["context-01-admin".to_owned()]);
    }

    #[test]
    fn slugify_path_token_normalizes_mixed_symbols() {
        assert_eq!(
            slugify_path_token("Inventory API__V2!!!"),
            "inventory-api__v2"
        );
    }

    #[test]
    fn validate_studio_name_rejects_invalid_chars() {
        let error = validate_studio_name("inventory studio")
            .expect_err("spaces should fail studio name validation");
        assert!(
            error
                .to_string()
                .contains("not cargo-safe or filesystem-safe")
        );
    }

    #[test]
    fn ensure_output_dir_is_empty_rejects_non_empty_directory() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let file_path = temp_dir.path().join("existing.txt");
        std::fs::write(&file_path, "hello").expect("temp file should write");

        let error = ensure_output_dir_is_empty(&PathBuf::from(temp_dir.path()))
            .expect_err("non-empty dir should fail");
        assert!(
            error
                .to_string()
                .contains("already exists and is not empty")
        );
    }
}
