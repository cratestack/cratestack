#![cfg(test)]
//! Tests for cross-kind type declaration collisions under `to_snake_case`
//! normalization (cratestack#429). Only `type`/`enum`/`model` share a
//! generated Rust namespace (see the doc comment on
//! `validate_type_declaration_collisions` in `snake_case_collisions.rs`),
//! so those three kind-pairs get negative (rejected) tests here. `mixin`
//! and `auth` declarations are metadata-only — their own declaration name
//! is never turned into a generated Rust identifier — so every pair
//! involving either of them is a *positive* control: per cratestack#429's
//! explicit acceptance criterion ("do not reject pairs that share no
//! generated symbol"), a normalized-name collision against a `mixin` or
//! `auth` block must continue to validate successfully.

use super::parse_schema;

#[test]
fn type_enum_collision_camel_vs_snake() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Address {
  street String
}

enum address {
  HOME
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect_err("type-enum collision should be rejected");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("address"), "error: {message}");
}

#[test]
fn type_model_collision_camel_vs_snake() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type UserProfile {
  bio String
}

model user_profile {
  id Int @id
  name String
}
"#,
    )
    .expect_err("type-model collision should be rejected");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("user_profile"), "error: {message}");
}

#[test]
fn enum_model_collision_camel_vs_snake() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

enum Status {
  ACTIVE
}

model status {
  id Int @id
  name String
}
"#,
    )
    .expect_err("enum-model collision should be rejected");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("status"), "error: {message}");
}

// `mixin` and `auth` declarations are metadata-only (see the module doc
// comment): neither one's own name is ever turned into a generated Rust
// identifier, so a normalized-name collision against either must *not* be
// rejected — rejecting it would be exactly the over-rejection
// cratestack#429's acceptance criteria rule out.

#[test]
fn safe_type_mixin_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Timestamp {
  createdAt DateTime
}

mixin timestamp {
  updatedAt DateTime
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("mixin shares no generated symbol with type — must not be rejected");
}

#[test]
fn safe_enum_mixin_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

enum Priority {
  HIGH
}

mixin priority {
  level Int
}

model Task {
  id Int @id
  name String
}
"#,
    )
    .expect("mixin shares no generated symbol with enum — must not be rejected");
}

#[test]
fn safe_model_mixin_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Document {
  id Int @id
  title String
}

mixin document {
  content String
}
"#,
    )
    .expect("mixin shares no generated symbol with model — must not be rejected");
}

#[test]
fn safe_type_auth_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Context {
  userId Int
}

auth context {
  id Int
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("auth shares no generated symbol with type — must not be rejected");
}

#[test]
fn safe_enum_auth_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

enum Role {
  ADMIN
}

auth role {
  id Int
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("auth shares no generated symbol with enum — must not be rejected");
}

#[test]
fn safe_model_auth_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Session {
  id Int @id
  token String
}

auth session {
  userId Int
}
"#,
    )
    .expect("auth shares no generated symbol with model — must not be rejected");
}

#[test]
fn safe_mixin_auth_collision_camel_vs_snake() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

mixin Metadata {
  createdAt DateTime
}

auth metadata {
  userId Int
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("mixin and auth are both metadata-only — must not be rejected");
}

#[test]
fn safe_type_enum_different_normalization() {
    // Different names that normalize differently should be allowed
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Address {
  street String
}

enum Status {
  ACTIVE
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("should validate successfully");
}

#[test]
fn safe_type_model_different_normalization() {
    // Different names that normalize differently should be allowed
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Address {
  street String
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("should validate successfully");
}

#[test]
fn safe_type_mixin_different_normalization() {
    // Different names that normalize differently should be allowed
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Address {
  street String
}

mixin Metadata {
  createdAt DateTime
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("should validate successfully");
}

#[test]
fn safe_enum_auth_different_normalization() {
    // Different names that normalize differently should be allowed
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

enum Status {
  ACTIVE
}

auth Session {
  userId Int
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("should validate successfully");
}

#[test]
fn multiple_collisions_reports_first() {
    // When multiple collisions exist, the first one is reported
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Foo {
  value String
}

enum foo {
  VARIANT
}

model bar {
  id Int @id
  name String
}

type Bar {
  content String
}
"#,
    )
    .expect_err("collision should be rejected");

    let message = err.to_string();
    // Should report the type-enum collision (first collision in source order)
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("foo"), "error: {message}");
}

#[test]
fn collision_with_underscores() {
    // myVar and my_var normalize to the same thing
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type myVar {
  value String
}

enum my_var {
  VARIANT
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect_err("underscore collision should be rejected");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("my_var"), "error: {message}");
}

#[test]
fn safe_underscore_handling() {
    // myVar and myOtherVar normalize differently
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type myVar {
  value String
}

enum myOtherVar {
  VARIANT
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect("should validate successfully");
}
