//! One row per op: the rules written on its declaration, analysed.

use cratestack_core::{ClientOp, ModelVerb, OpTarget, Procedure, Schema, client_ops};
use cratestack_policy::{analyse_op_policy, model_rule_expression, procedure_rule_expression};
use serde::Serialize;

/// One op of `contract ops`. Field order is the document's.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpRow {
    pub key: String,
    pub kind: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub roles: Vec<String>,
    pub requires_principal: bool,
    pub unanalysed: Vec<String>,
    pub contract_digest: String,
}

/// Every op of `schema`, sorted by key, each with its digest from `digest`.
pub(super) fn rows(schema: &Schema, digest: impl Fn(&str) -> String) -> Vec<OpRow> {
    client_ops(schema)
        .iter()
        .map(|op| row(op, digest(&op.key)))
        .collect()
}

fn row(op: &ClientOp<'_>, contract_digest: String) -> OpRow {
    let (kind, OpRules { allow, deny }) = match op.target {
        OpTarget::Procedure(procedure) => ("procedure".to_owned(), procedure_rules(procedure)),
        OpTarget::SubscriptionProcedure(procedure) => {
            ("subscription".to_owned(), procedure_rules(procedure))
        }
        OpTarget::Model(model, verb) => (
            format!("model.{}", verb.as_str()),
            model_rules(&model.attributes, verb),
        ),
    };
    let allow_text: Vec<&str> = allow.valid.iter().map(String::as_str).collect();
    let deny_text: Vec<&str> = deny.valid.iter().map(String::as_str).collect();
    let mut verdict = analyse_op_policy(&allow_text, &deny_text);
    // A directive whose own shape is invalid (the macros reject it) has no
    // expression to analyse: report its text, and for an `@allow` treat the
    // op as anonymous like any other rule that is not understood.
    verdict.unanalysed.extend(allow.malformed.iter().cloned());
    verdict.unanalysed.extend(deny.malformed.iter().cloned());
    OpRow {
        key: op.key.clone(),
        kind,
        requires_principal: verdict.requires_principal && allow.malformed.is_empty(),
        allow: allow.valid,
        deny: deny.valid,
        roles: verdict.roles,
        unanalysed: verdict.unanalysed,
        contract_digest,
    }
}

/// The `@allow` and `@deny` rules of one op.
#[derive(Default)]
struct Rules {
    valid: Vec<String>,
    malformed: Vec<String>,
}

struct OpRules {
    allow: Rules,
    deny: Rules,
}

impl OpRules {
    fn collect<'a>(
        attributes: impl Iterator<Item = &'a str> + Clone,
        extract: impl Fn(&'a str, &str) -> Option<Result<&'a str, String>>,
        (allow, deny): (&str, &str),
    ) -> Self {
        let pick = |directive: &str| {
            let mut rules = Rules::default();
            for raw in attributes.clone() {
                match extract(raw, directive) {
                    Some(Ok(expression)) => rules.valid.push(expression.to_owned()),
                    Some(Err(_)) => rules.malformed.push(raw.to_owned()),
                    None => {}
                }
            }
            rules
        };
        Self {
            allow: pick(allow),
            deny: pick(deny),
        }
    }
}

fn procedure_rules(procedure: &Procedure) -> OpRules {
    OpRules::collect(
        procedure.attributes.iter().map(|a| a.raw.as_str()),
        procedure_rule_expression,
        ("@allow", "@deny"),
    )
}

/// The model rules that decide `verb`: the slots the generated descriptor
/// reads for it (`macros/src/model/descriptor.rs`). `subscribe` filters its
/// events by the read policy, so it takes the `list` slot.
fn model_rules(attributes: &[cratestack_core::Attribute], verb: ModelVerb) -> OpRules {
    let actions: &[&str] = match verb {
        ModelVerb::List | ModelVerb::Subscribe => &["list", "read"],
        ModelVerb::Get => &["detail", "read"],
        ModelVerb::Create => &["create"],
        ModelVerb::Update => &["update"],
        ModelVerb::Delete => &["delete"],
    };
    OpRules::collect(
        attributes.iter().map(|a| a.raw.as_str()),
        |raw, directive| model_rule_expression(raw, directive, actions),
        ("@@allow", "@@deny"),
    )
}
