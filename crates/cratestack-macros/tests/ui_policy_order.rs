//! Compile-fail UI tests pinning *which* error a schema with two faulty policy
//! attributes reports first (cratestack#1388). The policy reading is shared
//! with `cratestack contract ops` (`cratestack-policy`), and an earlier
//! version of that sharing changed the lowering order, so a schema with two
//! mistakes reported a different one than it did before. The `.stderr`
//! snapshots here were produced on `origin/main` (`ab46b8f5`) and must stay
//! byte-identical:
//!
//! - `@@deny("read", bogus)` + `@@allow("detail", alsobogus)` -> `alsobogus`
//! - `@@allow("create", bogus1)` + `@@deny("read", bogus2)` -> `bogus1`
//!
//! The procedure case (`@authorize(bad)` + `@Allow(flag)` -> "invalid
//! @authorize") cannot go through a schema here, because the parser refuses
//! `@authorize(bad)` first; it is a unit test on the lowering,
//! `src/procedure/tests_policy_order.rs`.
//!
//! Fixture staging follows `ui_semantic.rs`; the staging dir is distinct so
//! parallel test binaries do not race.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_STAGING_DIR: &str = "/tmp/cratestack-macros-ui-policy-order-1388";

#[test]
fn policy_diagnostic_order_matches_main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = manifest_dir.join("tests/ui/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/ui/generated");
    let staging_dir = Path::new(FIXTURE_STAGING_DIR);
    fs::create_dir_all(staging_dir).expect("create fixture staging dir");

    let t = trybuild::TestCases::new();
    let cases = [
        (
            "policy_order_detail_allow_before_read_deny",
            "include_embedded_schema!({staged:?})",
        ),
        (
            "policy_order_create_allow_before_read_deny",
            "include_embedded_schema!({staged:?})",
        ),
    ];
    for (name, invocation) in cases {
        let source = manifest_dir.join(format!("tests/fixtures/{name}.cstack"));
        let staged = staging_dir.join(format!("{name}.cstack"));
        fs::copy(&source, &staged).expect("stage fixture");
        let staged = staged.to_str().expect("fixture path is UTF-8");
        let invocation = invocation.replace("{staged:?}", &format!("{staged:?}"));
        let body = format!("cratestack_macros::{invocation};\n\nfn main() {{}}\n");
        fs::write(generated_dir.join(format!("{name}.rs")), body).expect("write fixture");
        t.compile_fail(generated_dir.join(format!("{name}.rs")));
    }
}
