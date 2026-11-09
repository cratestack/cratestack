//! Keyset pagination in the generated TypeScript client (cratestack#1304):
//! string checks on the emitted types plus, where Node is available, a real
//! `tsc` pass over the generated package (default and `--swr`, REST and RPC)
//! with a usage file that only compiles if `PageInfo.nextCursor`,
//! `CursorPageInput`, `CursorPage<T>` and the `cursor` list-query field have
//! the intended types. The `cursor` wire encoding itself is proven against
//! the real server grammar in `rest_list_query_wire_format.rs` /
//! `rpc_list_query_wire_format.rs`.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

fn generate(fixture: &str, swr: bool) -> GeneratedTypeScriptPackage {
    generate_with(fixture, swr, false)
}

fn generate_with(fixture: &str, swr: bool, tanstack: bool) -> GeneratedTypeScriptPackage {
    let schema = cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
        .unwrap_or_else(|error| panic!("fixture {fixture} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "cursor-pagination-check".to_owned(),
            swr,
            tanstack,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture} swr={swr} should render: {error}"))
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|f| f.file_name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .contents
        .as_str()
}

#[test]
fn generated_types_carry_next_cursor_and_the_cursor_page_pair() {
    for fixture in ["cursor_pagination", "cursor_pagination_rpc"] {
        let package = generate(fixture, false);
        let models = file(&package, "src/models.ts");
        assert!(models.contains("nextCursor?: string | null;"), "{fixture}");
        assert!(models.contains("export interface CursorPageInput {"));
        assert!(models.contains("export interface CursorPage<T> {"));
        assert!(
            models.contains("page: CursorPageInput;"),
            "procedure args use the built-in: {fixture}"
        );
        let all: String = package.files.iter().map(|f| f.contents.as_str()).collect();
        assert!(
            all.contains("CursorPage<Entry>"),
            "{fixture}: procedure result type"
        );
    }
    let rest = generate("cursor_pagination", false);
    assert!(file(&rest, "src/queries.ts").contains("cursor?: string;"));
    assert!(file(&rest, "src/queries.ts").contains("output.cursor = query.cursor;"));
    let rpc = generate("cursor_pagination_rpc", false);
    assert!(file(&rpc, "src/queries.ts").contains("input.cursor = query.cursor;"));
}

#[test]
fn swr_layout_imports_the_cursor_types_it_uses() {
    for fixture in ["cursor_pagination", "cursor_pagination_rpc"] {
        let package = generate(fixture, true);
        let shared = file(&package, "src/swr/models/shared.ts");
        assert!(shared.contains("export interface CursorPageInput {"));
        assert!(shared.contains("export interface CursorPage<T> {"));
        assert!(shared.contains("nextCursor?: string | null;"));
    }
}

#[test]
fn generated_cursor_surface_type_checks() {
    if !["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }) {
        eprintln!("skipping generated_cursor_surface_type_checks: node/npm/npx not on PATH");
        return;
    }
    for (fixture, swr, tanstack) in [
        ("cursor_pagination", false, false),
        ("cursor_pagination", true, false),
        ("cursor_pagination", false, true),
        ("cursor_pagination_rpc", false, false),
        ("cursor_pagination_rpc", true, false),
        ("cursor_pagination_rpc", false, true),
    ] {
        let package = generate_with(fixture, swr, tanstack);
        let dir = tempfile::tempdir().expect("tempdir");
        for f in &package.files {
            let path = dir.path().join(&f.file_name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &f.contents).unwrap();
        }
        let types = if swr {
            "./swr/models/shared"
        } else {
            "./models"
        };
        let queries = if swr { "./swr/queries" } else { "./queries" };
        let list_query = if fixture.ends_with("rpc") {
            "CratestackRpcListQuery"
        } else {
            "CratestackFetchQuery"
        };
        fs::write(
            dir.path().join("src/cursor_usage.ts"),
            format!(
                r#"import type {{ CursorPage, CursorPageInput, PageInfo }} from "{types}";
import type {{ {list_query} }} from "{queries}";

export const start: CursorPageInput = {{ limit: 10, cursor: null }};
export const info: PageInfo = {{ limit: 10, offset: null, hasNextPage: true, hasPreviousPage: false, nextCursor: "x" }};
export const lastInfo: PageInfo = {{ limit: 10, offset: null, hasNextPage: false, hasPreviousPage: true }};
export const page: CursorPage<{{ id: number }}> = {{ items: [{{ id: 1 }}] }};
export const query: {list_query} = {{ limit: 10, cursor: page.nextCursor ?? "" }};
// @ts-expect-error cursor is a string, not a number
export const bad: {list_query} = {{ cursor: 1 }};
"#
            ),
        )
        .unwrap();
        let install = Command::new("npm")
            .args([
                "install",
                "--no-save",
                "--no-audit",
                "--no-fund",
                "typescript@5",
                "swr",
                "@tanstack/react-query",
                "react",
                "@types/react",
            ])
            .current_dir(dir.path())
            .output()
            .expect("run npm install");
        assert!(
            install.status.success(),
            "{fixture}: npm install failed: {}",
            String::from_utf8_lossy(&install.stderr)
        );
        let tsc = Command::new("npx")
            .args(["--yes", "tsc", "--noEmit", "-p", "tsconfig.json"])
            .current_dir(dir.path())
            .output()
            .expect("run npx tsc");
        assert!(
            tsc.status.success(),
            "{fixture} swr={swr} tanstack={tanstack}: tsc failed:\n{}\n{}",
            String::from_utf8_lossy(&tsc.stdout),
            String::from_utf8_lossy(&tsc.stderr)
        );
    }
}
