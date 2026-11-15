//! Which `@@allow` / `@@deny` slot of a model decides each verb.
//!
//! A model's rules are filed under an action (`@@allow("list", ..)`) and the
//! generated `ModelDescriptor` keeps one allow/deny pair per slot. Which slot
//! a verb reads is decided here once: the macros pick the rules they lower
//! and the descriptor field a generated feed reads from this table, and
//! `cratestack contract ops` picks the rules it analyses from it, so the
//! analysis cannot describe a different slot than the generated code reads.
//!
//! | verb | slot | actions whose rules fill it | descriptor fields |
//! |---|---|---|---|
//! | `list` | `List` | `list`, `read` | `read_allow_policies` / `read_deny_policies` |
//! | `get` | `Detail` | `detail`, `read` | `detail_allow_policies` / `detail_deny_policies` |
//! | `subscribe` | `Detail` | `detail`, `read` | `detail_allow_policies` / `detail_deny_policies` |
//! | `create` | `Create` | `create` | `create_allow_policies` / `create_deny_policies` |
//! | `update` | `Update` | `update` | `update_allow_policies` / `update_deny_policies` |
//! | `delete` | `Delete` | `delete` | `delete_allow_policies` / `delete_deny_policies` |
//!
//! `subscribe` reads `Detail`, not `List`: a feed decides each event the way
//! `get` decides one row (`record_readable`), at open and per event. A rule
//! filed under `all` fills every slot.

use cratestack_core::ModelVerb;

/// One allow/deny pair of a generated `ModelDescriptor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelPolicySlot {
    /// What `list` reads (the `list` and `read` actions).
    List,
    /// What `get` and `subscribe` read (the `detail` and `read` actions).
    Detail,
    /// What `create` reads.
    Create,
    /// What `update` reads.
    Update,
    /// What `delete` reads.
    Delete,
}

impl ModelPolicySlot {
    /// The actions whose `@@allow` / `@@deny` rules fill this slot, first
    /// being the one a malformed rule is reported against.
    pub const fn actions(self) -> &'static [&'static str] {
        match self {
            Self::List => &["list", "read"],
            Self::Detail => &["detail", "read"],
            Self::Create => &["create"],
            Self::Update => &["update"],
            Self::Delete => &["delete"],
        }
    }

    /// The `ModelDescriptor` field holding this slot's `@@allow` rules.
    pub const fn allow_field(self) -> &'static str {
        match self {
            Self::List => "read_allow_policies",
            Self::Detail => "detail_allow_policies",
            Self::Create => "create_allow_policies",
            Self::Update => "update_allow_policies",
            Self::Delete => "delete_allow_policies",
        }
    }

    /// The `ModelDescriptor` field holding this slot's `@@deny` rules.
    pub const fn deny_field(self) -> &'static str {
        match self {
            Self::List => "read_deny_policies",
            Self::Detail => "detail_deny_policies",
            Self::Create => "create_deny_policies",
            Self::Update => "update_deny_policies",
            Self::Delete => "delete_deny_policies",
        }
    }
}

/// The slots whose rules decide `verb`: the one table the macros and the
/// contract analysis both read. See the [module docs](self).
pub const fn model_verb_policy_slots(verb: ModelVerb) -> &'static [ModelPolicySlot] {
    match verb {
        ModelVerb::List => &[ModelPolicySlot::List],
        ModelVerb::Get | ModelVerb::Subscribe => &[ModelPolicySlot::Detail],
        ModelVerb::Create => &[ModelPolicySlot::Create],
        ModelVerb::Update => &[ModelPolicySlot::Update],
        ModelVerb::Delete => &[ModelPolicySlot::Delete],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_verb_reads_the_slot_the_runtime_does() {
        use ModelPolicySlot::{Create, Delete, Detail, List, Update};
        let table = [
            (ModelVerb::List, List),
            (ModelVerb::Get, Detail),
            (ModelVerb::Subscribe, Detail),
            (ModelVerb::Create, Create),
            (ModelVerb::Update, Update),
            (ModelVerb::Delete, Delete),
        ];
        for (verb, slot) in table {
            assert_eq!(model_verb_policy_slots(verb), [slot], "{verb:?}");
        }
    }

    #[test]
    fn the_read_slots_take_read_and_the_write_slots_do_not() {
        assert_eq!(ModelPolicySlot::List.actions(), ["list", "read"]);
        assert_eq!(ModelPolicySlot::Detail.actions(), ["detail", "read"]);
        assert_eq!(ModelPolicySlot::Update.actions(), ["update"]);
    }
}
