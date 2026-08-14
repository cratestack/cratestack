//! Read-side policy types (predicates / expressions / literals).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationQuantifier {
    ToOne,
    Some,
    Every,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyLiteral {
    Bool(bool),
    Int(i64),
    String(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPredicate {
    AuthNotNull,
    AuthIsNull,
    /// Lowered from `auth().isSystem()` (issue #486 / ADR 0038 blocker
    /// B1). Satisfied only by a `CratestackContext` minted through
    /// `cratestack_core::SystemContext`.
    ///
    /// This is a predicate a schema author writes down, not a bypass:
    /// it can only make a policy `TRUE` where an `@@allow`/`@@deny`
    /// clause names it. A model whose policies never mention it is
    /// entirely unaffected by system callers — the pre-existing
    /// default-deny / owner-scoped rules for that action still apply,
    /// which is the fail-closed property design constraint #2 asks for.
    AuthIsSystem,
    HasRole {
        role: &'static str,
    },
    InTenant {
        tenant_id: &'static str,
    },
    AuthFieldEqLiteral {
        auth_field: &'static str,
        value: PolicyLiteral,
    },
    AuthFieldNeLiteral {
        auth_field: &'static str,
        value: PolicyLiteral,
    },
    FieldIsTrue {
        column: &'static str,
    },
    FieldEqLiteral {
        column: &'static str,
        value: PolicyLiteral,
    },
    FieldNeLiteral {
        column: &'static str,
        value: PolicyLiteral,
    },
    FieldEqAuth {
        column: &'static str,
        auth_field: &'static str,
    },
    FieldNeAuth {
        column: &'static str,
        auth_field: &'static str,
    },
    Relation {
        quantifier: RelationQuantifier,
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        expr: &'static PolicyExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPolicy {
    pub expr: PolicyExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyExpr {
    Predicate(ReadPredicate),
    And(&'static [PolicyExpr]),
    Or(&'static [PolicyExpr]),
}
