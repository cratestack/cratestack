//! The `@allow`/`@deny` surface syntax, parsed once for every consumer.
//!
//! This is the half of policy handling that does not depend on a schema's
//! types: the expression grammar ([`ast`]), the shape of one term
//! ([`terms`]) and where a rule's text sits inside an attribute
//! ([`directive`]). `cratestack-macros` lowers what it yields to runtime
//! predicates; [`crate::analysis`] classifies it. Neither re-parses.

mod ast;
mod directive;
mod model_term;
mod procedure_term;
mod scan;
mod terms;
#[cfg(test)]
mod tests_ast;

pub use ast::{PolicyAst, parse_policy_ast};
pub use directive::{authorize_attribute_inner, model_rule_expression, procedure_rule_expression};
pub use model_term::{ModelTerm, read_model_term, split_in_term};
pub use procedure_term::{ProcedureTerm, read_procedure_term};
pub use scan::{find_unquoted, split_comparison};
pub use terms::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, is_auth_is_system_term, parse_builtin_policy_call,
    parse_string_literal,
};
