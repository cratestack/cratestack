//! cratestack#1381: the typed end reason ships in every package with a
//! subscription (REST and RPC alike), and in neither retrying hook's way.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const MODEL: &str = "model Note {\n  id Int @id\n  body String\n\n  @@allow(\"read\", true)\n  @@emit(created)\n  @@subscribe\n}\n";

fn files(source: &str, config: TypeScriptGeneratorConfig) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture should parse");
    generate_package(&schema, &config)
        .expect("package should render")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

fn contents<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(file, _)| file == name)
        .unwrap_or_else(|| panic!("{name} was not generated"))
        .1
        .as_str()
}

#[test]
fn rest_and_rpc_packages_expose_the_end_reason() {
    for source in [MODEL.to_owned(), format!("transport rpc\n\n{MODEL}")] {
        let files = files(&source, TypeScriptGeneratorConfig::default());
        let end = contents(&files, "src/subscription-end.ts");
        for reason in [
            "credential_expired",
            "stream_lifetime",
            "session_ended",
            "principal_suspended",
            "device_revoked",
            "revocation_resync",
        ] {
            assert!(end.contains(&format!("\"{reason}\"")), "{reason} missing");
        }
        let subscriptions = contents(&files, "src/subscriptions.ts");
        assert!(subscriptions.contains("get endReason(): StreamEndReason | undefined"));
        assert!(subscriptions.contains("type StreamEndReason"));
        assert!(subscriptions.contains("isTerminalEndReason"));
    }
}

#[test]
fn the_hooks_consult_the_retry_rule() {
    let tanstack = files(
        MODEL,
        TypeScriptGeneratorConfig {
            tanstack: true,
            ..TypeScriptGeneratorConfig::default()
        },
    );
    let query = contents(&tanstack, "src/react-query-subscriptions.ts");
    assert!(query.contains("shouldResubscribe(error)"));
    assert!(!query.contains("retry: false"));
    let swr = files(
        MODEL,
        TypeScriptGeneratorConfig {
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    );
    let hook = contents(&swr, "src/swr/subscriptions.hooks.ts");
    assert!(hook.contains("shouldResubscribe(error)"));
}
