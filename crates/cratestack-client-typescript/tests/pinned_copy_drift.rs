//! Drift guard between the two hand-maintained copies of the
//! `application/cbor-seq` boundary scanner (issue #277).
//!
//! There are two copies of this parser, and they are *not* the same file:
//!
//! * `templates/src/rpc-cbor-{item,seq}.ts.j2` — what actually ships into
//!   every generated `transport rpc` client. Verified on the Rust side
//!   only by `.contains(...)` assertions in `snapshot.rs`, which confirm
//!   symbols exist but say nothing about whether the algorithm is right.
//! * `packages/cratestack-ts-types/src/cbor-{item,seq}.ts` — the pinned
//!   npm copy, which is what all 24 behavioural vitest cases in
//!   `packages/cratestack-ts-types/tests/cbor-seq.test.ts` actually
//!   exercise (indefinite-length arrays/maps, truncation, byte-by-byte
//!   feeding, the tag-48900 sentinel, ...).
//!
//! So the rigorous tests cover the copy users never receive. As long as
//! the two stay identical that is fine — but nothing enforced it. A fix
//! applied to one copy and not the other would leave the vitest suite
//! green while every real generated client kept the bug. That is exactly
//! the "looks tested, isn't proven" gap #277's own Risks section named as
//! this work's highest risk, and it is worth a guard precisely because
//! this is the most failure-prone code in the package.
//!
//! Both `.j2` files contain **no jinja at all** (no `{{`, no `{%`) — they
//! are pure static TypeScript, so the generated output is byte-identical
//! to the template and this comparison is exact rather than approximate.
//!
//! The two copies legitimately differ in exactly three ways, all of which
//! this normalization strips: their doc-comment headers, their import
//! prologues (the generated package resolves `./runtime`/`./links`; the
//! npm package resolves `./index.js`), and biome's line-wrapping — which
//! adds trailing commas the unwrapped form does not have. Anything else
//! differing is drift.
//!
//! If this test fails: apply the change to *both* copies. Do not "fix" it
//! by loosening the normalization.

/// Strip everything that is allowed to differ, leaving only the logic:
/// block comments, line comments, the import/export-from prologue, and
/// all whitespace.
fn normalize(source: &str) -> String {
    let without_block_comments = strip_block_comments(source);

    let lines: Vec<&str> = without_block_comments
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .collect();

    // Drop the whole import/export-from prologue rather than matching
    // `import` line-by-line, so a formatter wrapping a long import across
    // several lines can't leave orphaned continuation lines behind and
    // trip a false failure.
    let body_start = lines
        .iter()
        .rposition(|line| line.contains("from \""))
        .map_or(0, |index| index + 1);

    let dense: String = lines[body_start..]
        .concat()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    strip_trailing_commas(dense)
}

/// Drop trailing commas before a closer. When a formatter wraps a long
/// call or parameter list across lines it adds one; unwrapped, it does
/// not. Both forms are the same code, so this is formatting, not logic —
/// exactly what this guard must see through. Looped because closers nest
/// (`{a:1,},)` needs two passes).
fn strip_trailing_commas(mut dense: String) -> String {
    loop {
        let collapsed = dense
            .replace(",)", ")")
            .replace(",]", "]")
            .replace(",}", "}");
        if collapsed == dense {
            return dense;
        }
        dense = collapsed;
    }
}

fn strip_block_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find("/*") {
        output.push_str(&rest[..open]);
        match rest[open..].find("*/") {
            Some(close) => rest = &rest[open + close + 2..],
            None => return output,
        }
    }
    output.push_str(rest);
    output
}

fn assert_copies_match(template_relative: &str, pinned_relative: &str) {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let template_path = manifest_dir.join(template_relative);
    let pinned_path = manifest_dir.join("../..").join(pinned_relative);

    let template = std::fs::read_to_string(&template_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", template_path.display()));
    let pinned = std::fs::read_to_string(&pinned_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", pinned_path.display()));

    assert!(
        !template.contains("{{") && !template.contains("{%"),
        "{template_relative} has gained jinja templating. This guard assumes these two \
         templates stay pure static TypeScript (that is why the comparison can be exact). \
         If templating is genuinely needed here, this test has to compare against real \
         *generated* output instead of the raw template."
    );

    assert_eq!(
        normalize(&template),
        normalize(&pinned),
        "the CBOR-seq boundary scanner has drifted between the copy that ships to users \
         ({template_relative}) and the pinned copy the vitest suite actually tests \
         ({pinned_relative}).\n\n\
         The behavioural tests exercise the pinned copy, so this drift means they are green \
         while generated clients may be broken. Apply the change to BOTH copies — do not \
         loosen this comparison. Comment headers and import prologues are already ignored."
    );
}

#[test]
fn cbor_item_walker_copies_do_not_drift() {
    assert_copies_match(
        "templates/src/rpc-cbor-item.ts.j2",
        "packages/cratestack-ts-types/src/cbor-item.ts",
    );
}

#[test]
fn cbor_seq_scanner_copies_do_not_drift() {
    assert_copies_match(
        "templates/src/rpc-cbor-seq.ts.j2",
        "packages/cratestack-ts-types/src/cbor-seq.ts",
    );
}

#[test]
fn normalize_ignores_comments_imports_and_formatting_but_not_logic() {
    // Guards the guard: prove the normalization actually collapses the
    // differences it claims to, and still catches a real logic change.
    let template = r#"
// header comment A
/* block
   comment */
import type { Codec } from "./runtime";
import { skipItem } from "./cbor-item";
function skipCount(bytes: Uint8Array, start: number): number {
  return start + 1;
}
"#;
    let pinned = r#"
// completely different header comment
import { skipItem } from "./cbor-item.js";
import type { Codec } from "./index.js";
function skipCount(
  bytes: Uint8Array,
  start: number,
): number {
  return start + 1;
}
"#;
    assert_eq!(normalize(template), normalize(pinned));

    let drifted = pinned.replace("start + 1", "start + 2");
    assert_ne!(
        normalize(template),
        normalize(&drifted),
        "normalization must still catch a real logic change"
    );
}
