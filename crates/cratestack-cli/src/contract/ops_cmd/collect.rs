//! One row per op: the rules written on its declaration, analysed.

use cratestack_core::{ClientOp, ModelVerb, OpTarget, Procedure, Schema, client_ops};
use cratestack_policy::{
    PolicyDialect, analyse_op_policy, audit_model_attributes, audit_procedure_attributes,
    model_rule_expression, model_verb_policy_slot, procedure_rule_expression,
};
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
        .into_iter()
        .map(|op| {
            let contract_digest = digest(&op.key);
            row(op, contract_digest)
        })
        .collect()
}

fn row(op: ClientOp<'_>, contract_digest: String) -> OpRow {
    let ClientOp { key, target, .. } = op;
    let dialect = match target {
        OpTarget::Model(..) => PolicyDialect::Model,
        _ => PolicyDialect::Procedure,
    };
    let (kind, rules, audit) = match target {
        OpTarget::Procedure(procedure) => (
            "procedure".to_owned(),
            procedure_rules(procedure),
            procedure_audit(procedure),
        ),
        OpTarget::SubscriptionProcedure(procedure) => (
            "subscription".to_owned(),
            procedure_rules(procedure),
            procedure_audit(procedure),
        ),
        OpTarget::Model(model, verb) => (
            format!("model.{}", verb.as_str()),
            model_rules(&model.attributes, verb),
            model_audit(model),
        ),
        // An op kind this build does not know how to read: reported, never
        // classified, so a consumer refuses it.
        _ => (
            "unknown".to_owned(),
            OpRules::default(),
            Err("op kind not understood by this build".to_owned()),
        ),
    };
    let OpRules { allow, deny } = rules;
    let allow_text: Vec<&str> = allow.valid.iter().map(String::as_str).collect();
    let deny_text: Vec<&str> = deny.valid.iter().map(String::as_str).collect();
    let mut verdict = analyse_op_policy(dialect, &allow_text, &deny_text);
    // A directive whose own shape is invalid (the macros reject it) has no
    // expression to analyse: report its text, and for an `@allow` treat the
    // op as anonymous like any other rule that is not understood.
    verdict.unanalysed.extend(allow.malformed.iter().cloned());
    verdict.unanalysed.extend(deny.malformed.iter().cloned());
    // The macros' own attribute audit refuses a rule it would drop (a
    // mis-spaced `@allow (x)`); the analysis cannot see that rule at all, so
    // the owner's verdict is not to be trusted: anonymous and listed.
    let audit_failed = audit.is_err();
    if let Err(message) = audit {
        verdict.unanalysed.push(message);
    }
    OpRow {
        key,
        kind,
        requires_principal: verdict.requires_principal
            && allow.malformed.is_empty()
            && !audit_failed,
        allow: allow.valid,
        deny: deny.valid,
        roles: verdict.roles,
        unanalysed: verdict.unanalysed,
        contract_digest,
    }
}

fn procedure_audit(procedure: &Procedure) -> Result<(), String> {
    audit_procedure_attributes(
        &format!("procedure `{}`", procedure.name),
        &procedure.attributes,
    )
}

fn model_audit(model: &cratestack_core::Model) -> Result<(), String> {
    // Every action the descriptor generates a slot for, as the macros audit.
    let actions = ["list", "read", "detail", "create", "update", "delete"];
    audit_model_attributes(
        &format!("model `{}`", model.name),
        &model.attributes,
        &actions,
    )
}

/// The `@allow` and `@deny` rules of one op.
#[derive(Default)]
struct Rules {
    valid: Vec<String>,
    malformed: Vec<String>,
}

#[derive(Default)]
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
/// reads for it, from `cratestack_policy::model_verb_policy_slot`, the same
/// table the macros lower from (`subscribe` reads `detail`, like `get`).
fn model_rules(attributes: &[cratestack_core::Attribute], verb: ModelVerb) -> OpRules {
    let actions = model_verb_policy_slot(verb).actions();
    OpRules::collect(
        attributes.iter().map(|a| a.raw.as_str()),
        |raw, directive| model_rule_expression(raw, directive, actions),
        ("@@allow", "@@deny"),
    )
}
