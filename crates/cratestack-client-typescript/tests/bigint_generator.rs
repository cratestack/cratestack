//! Generated-text locks for ADR 0019's `BigInt` arms in the TypeScript
//! generator. Behaviour is proven by `tests/bigint_round_trip.rs` and
//! `tests/native_cbor_bigint_encode.rs`, which run the generated code; this
//! file pins the specific catch-all arms that would otherwise degrade
//! silently into code that still compiles:
//!
//! * `types.rs`'s `other => other` would emit the name `BigInt`, which `tsc`
//!   accepts as the global JS wrapper *interface*, typing the field as the
//!   boxed object instead of the primitive;
//! * `find_many_views.rs` would omit the field from `<Model>Where`, so a
//!   client's filter key is never offered;
//! * `wire_shapes.rs`'s procedure-return arm would fall through to the
//!   `Shape` case, whose revival of a bare scalar is a no-op, leaving a
//!   string typed `bigint`.

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

fn generate(fixture: &str, config: TypeScriptGeneratorConfig) -> GeneratedTypeScriptPackage {
    let schema = cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
        .unwrap_or_else(|error| panic!("fixture {fixture} should parse: {error}"));
    generate_package(&schema, &config).expect("template should render")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    &package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package has no {name}"))
        .contents
}

fn rest() -> GeneratedTypeScriptPackage {
    generate(
        "bigint_scalar",
        TypeScriptGeneratorConfig {
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
}

fn rpc(native_cbor: bool) -> GeneratedTypeScriptPackage {
    generate(
        "bigint_scalar_rpc",
        TypeScriptGeneratorConfig {
            native_cbor,
            ..TypeScriptGeneratorConfig::default()
        },
    )
}

#[test]
fn a_bigint_field_is_the_primitive_bigint_not_the_global_wrapper_interface() {
    for models in [
        file(&rest(), "src/models.ts").to_owned(),
        file(&rest(), "src/swr/models/ledger.ts").to_owned(),
    ] {
        for expected in [
            "amountE8?: bigint;",
            "feeE8?: bigint | null;",
            "tiers?: bigint[];",
            "amountE8: bigint;",
            "tiers: bigint[];",
        ] {
            assert!(models.contains(expected), "missing `{expected}`:\n{models}");
        }
        for wrapper in [
            ": BigInt;",
            ": BigInt |",
            ": BigInt[]",
            "?: BigInt;",
            "?: BigInt |",
            "?: BigInt[",
        ] {
            assert!(
                !models.contains(wrapper),
                "a field fell through to the global `BigInt` interface (`{wrapper}`):\n{models}"
            );
        }
    }
    // A `BigInt @id` is a `bigint` in `get(id)` and in the create input.
    let client = file(&rest(), "src/client.ts").to_owned();
    assert!(client.contains("get(id: bigint, options"), "{client}");
    assert!(
        file(&rest(), "src/models.ts")
            .contains("export interface CreateCounterInput {\n  id: bigint;")
    );
}

#[test]
fn a_bigint_procedure_argument_and_return_are_typed_bigint() {
    let models = file(&rest(), "src/models.ts").to_owned();
    assert!(models.contains(
        "export interface ScaleArgs {\n  amountE8: bigint;\n  factorE8?: bigint | null;\n}"
    ));
    let client = file(&rest(), "src/client.ts").to_owned();
    for expected in [
        "balance(args: BalanceArgs, options: CratestackRequestConfig = {}): Promise<bigint>",
        "maybeBalance(args: MaybeBalanceArgs, options: CratestackRequestConfig = {}): Promise<bigint | null>",
        "history(args: HistoryArgs, options: CratestackRequestConfig = {}): Promise<bigint[]>",
    ] {
        assert!(client.contains(expected), "missing `{expected}`:\n{client}");
    }
}

#[test]
fn bigint_filters_are_comparable_bigint_and_reach_where_in_both_layouts() {
    let package = rest();
    let models = file(&package, "src/models.ts");
    assert!(models.contains("export type BigIntFilter = ComparableFilter<bigint>;"));
    assert!(models.contains("amountE8?: BigIntFilter;"), "{models}");
    assert!(models.contains("hitsE8?: BigIntFilter;"), "{models}");
    // A `BigInt @id` filters too, and the String field of the same name does not.
    assert!(
        models.contains("export interface CounterWhere {\n  id?: BigIntFilter;"),
        "{models}"
    );
    assert!(models.contains("export interface EntryWhere {\n  id?: StringFilter;\n  ledgerId?: StringFilter;\n  deltaE8?: BigIntFilter;\n  amountE8?: StringFilter;"), "{models}");

    let shared = file(&package, "src/swr/models/shared.ts");
    assert!(shared.contains("export type BigIntFilter = ComparableFilter<bigint>;"));
    let ledger = file(&package, "src/swr/models/ledger.ts");
    assert!(ledger.contains("amountE8?: BigIntFilter;"), "{ledger}");
    assert!(
        ledger.contains("import type { BigIntFilter, "),
        "the swr per-model file must import BigIntFilter from ./shared.js:\n{ledger}"
    );
}

#[test]
fn revival_registries_name_the_bigint_keys_per_type_in_both_layouts() {
    let package = rest();
    for path in ["src/models.ts", "src/swr/models/shared.ts"] {
        let text = file(&package, path);
        for expected in [
            "Ledger: { decimalKeys: [], bigintKeys: ['amountE8', 'feeE8', 'tiers'],",
            "Entry: { decimalKeys: [], bigintKeys: ['deltaE8'],",
            "Counter: { decimalKeys: [], bigintKeys: ['id', 'hitsE8'],",
            "Totals: { decimalKeys: [], bigintKeys: ['grossE8', 'history'],",
            "readonly bigintKeys: readonly string[];",
            "} else if (shape.bigintKeys.includes(key)) {",
        ] {
            assert!(
                text.contains(expected),
                "{path} is missing `{expected}`:\n{text}"
            );
        }
    }
}

#[test]
fn bare_bigint_procedure_returns_ask_for_the_bigint_revival_not_the_shape_no_op() {
    let package = rest();
    for (path, signature) in [
        ("src/client.ts", "reviveWireScalar(value, \"bigint\")"),
        (
            "src/swr/procedures.ts",
            "reviveWireScalar(value, \"bigint\")",
        ),
    ] {
        let text = file(&package, path);
        assert_eq!(
            text.matches(signature).count(),
            // balance, maybeBalance, history, scale
            4,
            "{path} should revive all four bare-bigint procedures:\n{text}"
        );
    }
    let rpc = rpc(true);
    assert_eq!(
        file(&rpc, "src/client.ts")
            .matches("reviveWireScalar(value, \"bigint\")")
            .count(),
        4
    );
    for path in ["src/models.ts", "src/swr/models/shared.ts"] {
        let text = file(&package, path);
        assert!(
            text.contains("if (kind === \"bigint\") {"),
            "{path} lacks the bigint scalar kind"
        );
    }
}

#[test]
fn every_json_encode_path_converts_a_bigint_before_json_stringify() {
    let package = rest();
    // The REST body and the REST object-valued query entry share one walk.
    let runtime = file(&package, "src/runtime.ts");
    assert!(runtime.contains("body = JSON.stringify(encodeBinaryAsJson(options.body));"));
    assert!(runtime.contains("searchParams.set(key, JSON.stringify(encodeBinaryAsJson(value)));"));
    assert!(
        !runtime.contains("searchParams.set(key, JSON.stringify(value));"),
        "the REST query object path went back to a bare JSON.stringify:\n{runtime}"
    );

    let models = file(&package, "src/models.ts");
    assert!(models.contains("if (typeof value === \"bigint\" || value instanceof Decimal) {"));
    assert!(models.contains("if (typeof value === \"bigint\") {\n    return value.toString();"));

    // RPC `computedParams` go through the shared helper, never a bare stringify.
    for (path, package) in [
        ("src/queries.ts", rpc(true)),
        ("src/client.ts", rpc(true)),
        (
            "src/queries.ts",
            generate(
                "bigint_computed_params_rpc",
                TypeScriptGeneratorConfig::default(),
            ),
        ),
        (
            "src/client.ts",
            generate(
                "bigint_computed_params_rpc",
                TypeScriptGeneratorConfig::default(),
            ),
        ),
        (
            "src/swr/models/quote.ts",
            generate(
                "bigint_computed_params_rpc",
                TypeScriptGeneratorConfig {
                    swr: true,
                    ..TypeScriptGeneratorConfig::default()
                },
            ),
        ),
    ] {
        let text = file(&package, path);
        assert!(
            !text.contains("JSON.stringify(options.computedParams)")
                && !text.contains("JSON.stringify(query.computedParams)"),
            "{path} stringifies computedParams without the bigint conversion:\n{text}"
        );
    }
    let computed = generate(
        "bigint_computed_params_rpc",
        TypeScriptGeneratorConfig::default(),
    );
    assert_eq!(
        file(&computed, "src/client.ts")
            .matches("encodeComputedParams(options.computedParams)")
            .count(),
        1
    );
    assert!(
        file(&computed, "src/queries.ts").contains("encodeComputedParams(query.computedParams)")
    );
}

#[test]
fn the_codecs_convert_a_bigint_themselves_so_link_batch_cannot_skip_it() {
    // JSON: the codec's own encode walks the value.
    let json = rpc(false);
    let runtime = file(&json, "src/runtime.ts");
    assert!(runtime.contains("return JSON.stringify(encodeBinaryAsJson(value) ?? null);"));
    assert!(
        !runtime.contains("withWireEncoding"),
        "a --no-native-cbor build must not mention the native codec wrapper:\n{runtime}"
    );

    // Native CBOR: the resolved `@cratestack/cbor` codec is wrapped, once,
    // by a memoized wrapper, and the retry-on-rejection shape is intact.
    let native = rpc(true);
    let runtime = file(&native, "src/runtime.ts");
    assert!(
        runtime.contains(
            "encode: (value: unknown): BodyInit => codec.encode(encodeWireFields(value)),"
        )
    );
    assert!(runtime.contains(
        "const wireEncodingCodecs = new WeakMap<CratestackRpcCodec, CratestackRpcCodec>();"
    ));
    assert!(
        runtime.contains("}).then(withWireEncoding));"),
        "the native codec is no longer wrapped on resolution:\n{runtime}"
    );
    assert_eq!(runtime.matches("createCborCodec().catch(").count(), 1);
}
