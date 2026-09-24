//! A hand-written tool table, standing in for the generated one, so this
//! crate's protocol behaviour is tested without the schema macro. What the
//! generated table adds — the policy call through `invoke_with_db` — is
//! tested where it is generated, in `cratestack-api`'s and `cratestack-pg`'s
//! `tests/mcp_*.rs`.

#![allow(dead_code)] // Each test binary uses a different subset.

pub mod client;
pub mod counting;
pub mod failing;
pub mod http_app;
pub mod resources;
pub mod stores;
pub mod token;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind};
use cratestack_mcp::{ArgumentsError, McpTools, ToolDescriptor, decode_arguments};
use serde::Deserialize;
use serde_json::{Value, json};

static READ_OP: OpDescriptor = op("procedure.echo", true);
static WRITE_OP: OpDescriptor = op("procedure.transfer", false);

const fn op(op_id: &'static str, idempotent_by_default: bool) -> OpDescriptor {
    OpDescriptor {
        op_id,
        kind: OpKind::Unary,
        input_ty: "",
        output_ty: "",
        idempotent_by_default,
        rate_limited_by_default: true,
        auth_required: false,
    }
}

pub const ECHO_INPUT: &str =
    r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#;
pub const ECHO_OUTPUT: &str =
    r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#;
pub const TRANSFER_INPUT: &str =
    r#"{"type":"object","properties":{"amount":{"type":"integer"}},"required":["amount"]}"#;

pub static TOOLS: [ToolDescriptor; 2] = [
    ToolDescriptor::new(
        "echo",
        Some("Echo the text back."),
        ECHO_INPUT,
        Some(ECHO_OUTPUT),
        true,
        &READ_OP,
    ),
    ToolDescriptor::new("transfer", None, TRANSFER_INPUT, None, false, &WRITE_OP),
];

#[derive(Deserialize)]
pub struct EchoArgs {
    text: String,
}

#[derive(Deserialize)]
pub struct TransferArgs {
    amount: i64,
}

pub enum Call {
    Echo(EchoArgs),
    Transfer(TransferArgs),
}

/// Counts executions, which is the only honest way to assert that a
/// refused call never ran.
#[derive(Clone, Default)]
pub struct FakeTools {
    pub runs: Arc<AtomicUsize>,
}

impl FakeTools {
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl McpTools for FakeTools {
    type Call = Call;

    fn tools(&self) -> &'static [ToolDescriptor] {
        &TOOLS
    }

    fn decode(&self, tool: &str, arguments: Value) -> Result<Call, ArgumentsError> {
        match tool {
            "echo" => decode_arguments(arguments).map(Call::Echo),
            "transfer" => decode_arguments(arguments).map(Call::Transfer),
            other => Err(ArgumentsError::new(format!("no tool `{other}`"))),
        }
    }

    async fn execute(&self, call: Call, ctx: &CratestackContext) -> Result<Value, CratestackError> {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        match call {
            Call::Echo(args) => Ok(json!({ "text": args.text })),
            Call::Transfer(args) if args.amount < 0 => Err(CratestackError::Database(
                "operator-only detail: relation \"ledger\" is locked".to_owned(),
            )),
            // The run number makes a replay distinguishable from a rerun.
            Call::Transfer(args) => Ok(json!([args.amount, run, ctx.principal_actor_id()])),
        }
    }
}

pub fn user(id: &str) -> CratestackContext {
    CratestackContext::authenticated([(
        "id".to_owned(),
        cratestack_core::Value::String(id.to_owned()),
    )])
}
