# cratestack-policy

Canonical policy literals, predicates, and procedure-policy evaluation types.

## Overview

`cratestack-policy` defines the data shapes that generated code emits for `@@allow` / `@@deny` model attributes and procedure policies. The generated SQLx code translates `ReadPredicate` variants into SQL fragments; procedure dispatch evaluates `ProcedurePredicate` variants in Rust against the `CoolContext` and the procedure arguments.

The types here are `Copy + 'static` so generated code can embed them as `const`s.

## Installation

```toml
[dependencies]
cratestack-policy = "0.2.2"
```

## Schema Surface

The most common entry point is the schema:

```cstack
model Post {
  id String @id
  title String
  authorId String
  published Boolean

  @@allow("read", auth() != null)
  @@allow("update", auth().id == authorId)
  @@allow("delete", auth().role == "admin")
}
```

`include_schema!` lowers each `@@allow` clause into a `ReadPolicy { expr: PolicyExpr }` constant.

## Literals

```rust
pub enum PolicyLiteral {
    Bool(bool),
    Int(i64),
    String(&'static str),
}
```

## Read Predicates

`ReadPredicate` is the leaf type embedded in `PolicyExpr::Predicate`. The variants:

- `AuthNotNull` / `AuthIsNull`
- `HasRole { role }`
- `InTenant { tenant_id }`
- `AuthFieldEqLiteral { auth_field, value }`
- `AuthFieldNeLiteral { auth_field, value }`
- `FieldIsTrue { column }`
- `FieldEqLiteral { column, value }`
- `FieldNeLiteral { column, value }`
- `FieldEqAuth { column, auth_field }`
- `FieldNeAuth { column, auth_field }`
- `Relation { quantifier, parent_table, parent_column, related_table, related_column, expr }`

`RelationQuantifier` variants: `ToOne`, `Some`, `Every`, `None`.

## Compound Expressions

```rust
use cratestack_policy::{PolicyExpr, ReadPolicy, ReadPredicate};

const POLICY: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::And(&[
        PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
        PolicyExpr::Or(&[
            PolicyExpr::Predicate(ReadPredicate::HasRole { role: "admin" }),
            PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
                column: "ownerId",
                auth_field: "id",
            }),
        ]),
    ]),
};
```

`PolicyExpr` variants: `Predicate(ReadPredicate)`, `And(&'static [PolicyExpr])`, `Or(&'static [PolicyExpr])`.

## Procedure Policies

Procedures use a parallel type tree because the inputs differ — there is no row, only the call arguments:

- `ProcedurePolicyLiteral` (same shape as `PolicyLiteral`)
- `ProcedurePredicate` variants include the row-free predicates (`AuthNotNull`, `HasRole`, etc.) plus `InputFieldIsTrue`, `InputFieldEqLiteral`, `InputFieldNeLiteral`, `InputFieldEqAuth`, `InputFieldNeAuth`, `InputFieldEqInput`, `InputFieldNeInput`
- `ProcedurePolicyExpr::{Predicate, And, Or}`
- `ProcedurePolicy { expr: ProcedurePolicyExpr }`

The `ProcedureArgs` trait lets the evaluator look up `args.<path>` values; `authorize_procedure` runs the policy against a `CoolContext` and a `ProcedureArgs`.

```rust
use cratestack_policy::{ProcedurePolicy, ProcedureArgs, authorize_procedure};

fn check<A: ProcedureArgs>(ctx: &cratestack_core::CoolContext, args: &A, policy: &ProcedurePolicy)
    -> Result<(), cratestack_core::CoolError>
{
    authorize_procedure(ctx, args, policy)
}
```

The crate also exposes `context_has_role` and `context_in_tenant` helpers for the procedure-level checks.

## See Also

- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- `cratestack-sqlx` — where `ReadPredicate` lowers into SQL
- `cratestack-macros` — emits these constants from schema attributes

## License

MIT
