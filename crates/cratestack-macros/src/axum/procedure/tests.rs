//! Regression guard for cratestack#283: a `T[]`-returning procedure with
//! no `@stream` attribute must generate the exact same axum dispatch
//! tokens `generate_procedure_axum_handler` generated before genuinely
//! incremental streaming existed — byte-identical, not just
//! "equivalent". `PINNED_NON_STREAM_DISPATCH` below is the exact
//! `.to_string()` output captured from this function *before* any
//! `@stream`-aware branching was added to it or to the invoke-call /
//! dispatch-tail helpers it composes — it has not moved since (except
//! once, deliberately, for #415 — see that constant's own doc comment),
//! so this test still passing is the "diff of zero lines" proof this
//! ticket's acceptance criteria ask for (mirrors `crate::procedure::tests`,
//! the analogous guard for cratestack#282's trait-method change).

use super::generate_procedure_axum_handler;

const LIST_RETURNING_SCHEMA: &str = r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
"#;

const STREAM_SCHEMA: &str = r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
"#;

fn parse_first_procedure(source: &str) -> cratestack_core::Procedure {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .procedures
        .remove(0)
}

/// Captured via `generated.to_string()` for `LIST_RETURNING_SCHEMA`
/// against the pre-cratestack#283 implementation. Do not "fix" this
/// string when touching stream-dispatch codegen — if this test fails
/// after such a change, the change broke the non-breaking guarantee
/// this ticket's acceptance criteria require ("non-`@stream` `T[]`
/// procedures ... byte-identical to today").
///
/// **Deliberately re-pinned once, for #415.** `enrich_context_from_headers`'s
/// call site here now threads `client_ip_ctx: ClientIpContext` (and the
/// generated handler/dispatch fn signatures carry the new parameter) — a
/// real, intentional change to every generated dispatch fn's tokens, unrelated
/// to cratestack#283's stream-vs-buffered distinction this test actually
/// guards. This constant was regenerated from the current (correct)
/// `generate_procedure_axum_handler` output specifically to restore that
/// guarantee for future stream-dispatch changes, not to paper over one.
const PINNED_NON_STREAM_DISPATCH: &str = "async fn handle_ticks < R , C , Auth > (State (state) : State < ProcedureRouterState < R , C , Auth >> , headers : HeaderMap , client_ip_ctx : ClientIpContext , body : Bytes ,) -> Response where R : super :: procedures :: ProcedureRegistry , C : HttpTransport , Auth : :: cratestack :: AuthProvider , { let canonical_body = body . clone () ; handle_ticks_dispatch (state , CanonicalRequest { method : \"POST\" , path : \"/$procs/ticks\" , query : None , body : canonical_body . as_ref () , } , headers , client_ip_ctx , body ,) . await } pub (super) async fn handle_ticks_dispatch < R , C , Auth > (state : ProcedureRouterState < R , C , Auth > , canonical : CanonicalRequest < '_ > , headers : HeaderMap , client_ip_ctx : ClientIpContext , body : Bytes ,) -> Response where R : super :: procedures :: ProcedureRegistry , C : HttpTransport , Auth : :: cratestack :: AuthProvider , { const CAPABILITIES : :: cratestack :: RouteTransportCapabilities = :: cratestack :: RouteTransportCapabilities { request_types : & [\"application/cbor\" , \"application/json\"] , response_types : & [\"application/cbor\" , \"application/json\" , :: cratestack :: CBOR_SEQUENCE_CONTENT_TYPE ,] , default_response_type : \"application/cbor\" , supports_sequence_response : true , } ; let canonical_route = canonical . path ; let span = :: cratestack :: tracing :: info_span ! (\"cratestack_procedure_route\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" ,) ; let _span_guard = span . enter () ; let started = :: std :: time :: Instant :: now () ; if let Err (error) = :: cratestack :: validate_transport_request_headers_for (& state . codec , & headers , & CAPABILITIES) { :: cratestack :: tracing :: warn ! (target : \"cratestack\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" , cratestack_error = error . code () , cratestack_detail = error . detail () . unwrap_or (\"\") , \"cratestack procedure preflight failed\") ; let result : Result < super :: procedures :: ticks :: Output , :: cratestack :: CoolError > = Err (error) ; return :: cratestack :: encode_transport_sequence_result_with_status_for (& state . codec , & headers , & CAPABILITIES , axum :: http :: StatusCode :: OK , result) ; } let request = request_context (canonical . method , canonical . path , canonical . query , & headers , canonical . body) ; let ctx = match state . auth_provider . authenticate (& request) . await { Ok (ctx) => :: cratestack :: enrich_context_from_headers (ctx , & headers , client_ip_ctx . trusted_proxy . as_ref () , client_ip_ctx . peer) , Err (error) => { let error : :: cratestack :: CoolError = error . into () ; :: cratestack :: tracing :: warn ! (target : \"cratestack\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" , cratestack_error = error . code () , cratestack_detail = error . detail () . unwrap_or (\"\") , \"cratestack procedure auth failed\") ; let result : Result < super :: procedures :: ticks :: Output , :: cratestack :: CoolError > = Err (error) ; return :: cratestack :: encode_transport_sequence_result_with_status_for (& state . codec , & headers , & CAPABILITIES , axum :: http :: StatusCode :: OK , result) ; } } ; let args = match :: cratestack :: decode_transport_request_for :: < _ , super :: procedures :: ticks :: Args > (& state . codec , & headers , & CAPABILITIES , & body) { Ok (args) => args , Err (error) => { :: cratestack :: tracing :: warn ! (target : \"cratestack\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" , cratestack_error = error . code () , cratestack_detail = error . detail () . unwrap_or (\"\") , \"cratestack procedure decode failed\") ; let result : Result < super :: procedures :: ticks :: Output , :: cratestack :: CoolError > = Err (error) ; return :: cratestack :: encode_transport_sequence_result_with_status_for (& state . codec , & headers , & CAPABILITIES , axum :: http :: StatusCode :: OK , result) ; } } ; let registry = state . registry . clone () ; let db = state . db . clone () ; let auth_db = db . clone () ; let call_args = args . clone () ; let call_ctx = ctx . clone () ; let result = super :: procedures :: ticks :: invoke_with_db (& auth_db , & args , & ctx , || async move { registry . ticks (& db , & call_ctx , call_args) . await }) . await ; match & result { Ok (_) => :: cratestack :: tracing :: info ! (target : \"cratestack\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" , cratestack_authenticated = ctx . is_authenticated () , cratestack_duration_ms = started . elapsed () . as_millis () as u64 , cratestack_request_id = ctx . request_id () . unwrap_or (\"\") , \"cratestack procedure route completed\" ,) , Err (error) => :: cratestack :: tracing :: warn ! (target : \"cratestack\" , cratestack_route = canonical_route , cratestack_procedure = \"ticks\" , cratestack_operation = \"procedure\" , cratestack_authenticated = ctx . is_authenticated () , cratestack_error = error . code () , cratestack_detail = error . detail () . unwrap_or (\"\") , cratestack_duration_ms = started . elapsed () . as_millis () as u64 , cratestack_request_id = ctx . request_id () . unwrap_or (\"\") , \"cratestack procedure route failed\" ,) , } let mut response = :: cratestack :: encode_transport_sequence_result_with_status_for (& state . codec , & headers , & CAPABILITIES , axum :: http :: StatusCode :: OK , result) ; response }";

#[test]
fn non_stream_list_procedure_dispatch_is_unchanged() {
    let procedure = parse_first_procedure(LIST_RETURNING_SCHEMA);
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert_eq!(
        generated, PINNED_NON_STREAM_DISPATCH,
        "non-`@stream` `T[]` procedure dispatch tokens changed — cratestack#283 must not \
         alter buffered sequence-response behavior"
    );
}

/// `@stream` dispatch must diverge from the buffered baseline (it calls
/// the new async stream encoder instead, and doesn't collect into a
/// `Vec` before invoking it) — this just pins that the two code paths
/// are in fact different, catching an accidental no-op branch.
#[test]
fn stream_list_procedure_dispatch_differs_from_buffered_baseline() {
    let procedure = parse_first_procedure(STREAM_SCHEMA);
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert_ne!(generated, PINNED_NON_STREAM_DISPATCH);
    assert!(generated.contains("encode_transport_stream_result_with_status_for"));
    assert!(!generated.contains("try_collect"));
}

/// cratestack#407: a unary procedure's `@status(202)` attribute threads
/// through into `encode_transport_result_with_status_for`'s `success_status`
/// argument, replacing the hardcoded `StatusCode::OK` literal.
#[test]
fn status_attribute_threads_through_for_unary_procedure() {
    let procedure = parse_first_procedure(
        r#"
type PingArgs {
  nonce String
}

type Pong {
  nonce String
}

mutation procedure submit(args: PingArgs): Pong
  @status(202)
"#,
    );
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert!(
        generated.contains("encode_transport_result_with_status_for"),
        "expected the unary (non-list) encoder: {generated}"
    );
    assert!(
        generated.contains("StatusCode :: from_u16 (202u16)"),
        "expected the declared @status(202) to thread through: {generated}"
    );
}

/// cratestack#407: the same threading applies to the `TypeArity::List`
/// branch (`encode_transport_sequence_result_with_status_for`), not just
/// the unary one.
#[test]
fn status_attribute_threads_through_for_list_procedure() {
    let procedure = parse_first_procedure(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @status(201)
"#,
    );
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert!(
        generated.contains("encode_transport_sequence_result_with_status_for"),
        "expected the list encoder: {generated}"
    );
    assert!(
        generated.contains("StatusCode :: from_u16 (201u16)"),
        "expected the declared @status(201) to thread through: {generated}"
    );
}

/// cratestack#407: absent `@status`, the generated handler still emits the
/// literal `StatusCode::OK` — fully backward compatible, opt-in only.
#[test]
fn absent_status_attribute_defaults_to_status_code_ok() {
    let procedure = parse_first_procedure(
        r#"
type PingArgs {
  nonce String
}

type Pong {
  nonce String
}

procedure ping(args: PingArgs): Pong
"#,
    );
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert!(
        generated.contains("StatusCode :: OK"),
        "expected the default StatusCode::OK when @status is absent: {generated}"
    );
}

/// cratestack#407 follow-up: `@status` was threaded into the unary and
/// `TypeArity::List` branches, but `procedure_dispatch_tail_tokens`
/// (the `@stream` branch) discarded it and re-derived its own call with
/// a hardcoded `StatusCode::OK` — so `@stream` + `@status(202)` silently
/// no-opped instead of erroring or working. This pins that the declared
/// status now reaches `encode_transport_stream_result_with_status_for`'s
/// `success_status` argument too, not just the two buffered encoders.
#[test]
fn status_attribute_threads_through_for_stream_procedure() {
    let procedure = parse_first_procedure(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
  @status(202)
"#,
    );
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert!(
        generated.contains("encode_transport_stream_result_with_status_for"),
        "expected the stream encoder: {generated}"
    );
    assert!(
        generated.contains("StatusCode :: from_u16 (202u16)"),
        "expected the declared @status(202) to thread through to the stream branch: \
         {generated}"
    );
    assert!(
        !generated.contains(
            "encode_transport_stream_result_with_status_for (& state . codec , & headers , & \
             CAPABILITIES , axum :: http :: StatusCode :: OK , result ,) . await"
        ),
        "the stream branch must not fall back to a hardcoded StatusCode::OK when @status is \
         present: {generated}"
    );
}

/// cratestack#407 follow-up: absent `@status`, a `@stream` procedure's
/// dispatch tail still emits the same literal `StatusCode::OK` it did
/// before — this is the backward-compatibility counterpart to the test
/// above, proving the fix only changes behavior when `@status` is
/// actually declared.
#[test]
fn absent_status_attribute_defaults_to_status_code_ok_for_stream_procedure() {
    let procedure = parse_first_procedure(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
"#,
    );
    let generated = generate_procedure_axum_handler(&procedure)
        .expect("codegen should succeed")
        .to_string();
    assert!(
        generated.contains(
            "encode_transport_stream_result_with_status_for (& state . codec , & headers , & \
             CAPABILITIES , axum :: http :: StatusCode :: OK , result ,) . await"
        ),
        "expected the default StatusCode::OK in the stream branch when @status is absent: \
         {generated}"
    );
}
