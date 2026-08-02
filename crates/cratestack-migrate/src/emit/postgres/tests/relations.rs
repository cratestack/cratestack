//! Regression coverage for issue #260: a declared `@relation` must
//! produce a real `FOREIGN KEY` constraint, not just a same-named
//! column with no referential integrity. Split by topic into sibling
//! submodules to stay under the 200-LoC budget.

mod existing_table;
mod new_table;
