use super::{parse_relation_attribute, parse_schema};
use cratestack_core::TransportStyle;

#[test]
fn parses_and_validates_initial_schema_subset() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth UserAuth {
  id Int
  role String
}

model User {
  id Int @id
  email String @unique
  role String

  @@allow("read", auth() != null)
}

type PublishPostInput {
  postId Int
}

mutation procedure publishPost(args: PublishPostInput): User
  @allow(auth().role == "admin")
"#,
    )
    .expect("schema should parse");

    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.types.len(), 1);
    assert_eq!(schema.procedures.len(), 1);
}

#[test]
fn parses_enums_and_allows_enum_type_references() {
    let schema = parse_schema(
        r#"
enum Role {
  admin
  member
}

auth SessionUser {
  role Role
}

model User {
  id Int @id
  role Role
}

type PublicUser {
  role Role
}

procedure getUsers(role: Role?): User
"#,
    )
    .expect("schema with enums should parse");

    assert_eq!(schema.enums.len(), 1);
    assert_eq!(schema.enums[0].name, "Role");
    assert_eq!(
        schema.enums[0]
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["admin", "member"]
    );
    assert_eq!(
        schema.auth.as_ref().expect("auth block").fields[0].ty.name,
        "Role"
    );
    assert_eq!(schema.models[0].fields[1].ty.name, "Role");
    assert_eq!(schema.types[0].fields[0].ty.name, "Role");
    assert_eq!(schema.procedures[0].args[0].ty.name, "Role");
}

#[test]
fn rejects_models_without_primary_keys() {
    let error = parse_schema(
        r#"
model User {
  email String
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("missing an @id field"));
}

#[test]
fn rejects_relation_fields_without_explicit_relation_metadata() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("must declare @relation"));
}

#[test]
fn rejects_relations_with_unknown_local_fields() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[ownerId],references:[id])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("unknown local field `ownerId`"));
}

#[test]
fn rejects_relations_with_unknown_target_fields() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[userId])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(error.to_string().contains("unknown target field `userId`"));
}

#[test]
fn rejects_relations_with_incompatible_scalar_reference_types() {
    let error = parse_schema(
        r#"
model User {
  id String @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#,
    )
    .expect_err("schema should fail validation");

    assert!(
        error
            .to_string()
            .contains("links incompatible scalar types")
    );
}

#[test]
fn accepts_custom_fields_on_types() {
    let schema = parse_schema(
        r#"
type Image {
  storageKey String
  thumbnailUrl String @custom
}
"#,
    )
    .expect("type custom fields should parse");

    assert_eq!(schema.types[0].fields[1].attributes[0].raw, "@custom");
}

#[test]
fn rejects_duplicate_enum_variants() {
    let error = parse_schema(
        r#"
enum Role {
  admin
  admin
}
"#,
    )
    .expect_err("duplicate enum variants should fail validation");

    assert!(
        error
            .to_string()
            .contains("duplicate variant `admin` on enum `Role`")
    );
}

#[test]
fn accepts_model_emit_attribute() {
    let schema = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@emit(created, deleted)
}
"#,
    )
    .expect("model emit attribute should parse");

    assert_eq!(
        schema.models[0].attributes[0].raw,
        "@@emit(created, deleted)"
    );
}

#[test]
fn rejects_invalid_model_emit_attribute_operation() {
    let error = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@emit(created, archived)
}
"#,
    )
    .expect_err("unknown event operation should fail validation");

    assert!(
        error
            .to_string()
            .contains("unsupported event operation `archived`")
    );
}

#[test]
fn preserves_leading_doc_comments_on_declarations_and_fields() {
    let schema = parse_schema(
        r#"
/// User docs
model User {
  /// Identifier docs
  id Int @id
  /// Email docs
  email String
}

/// Feed docs
procedure getFeed(): User
"#,
    )
    .expect("schema with docs should parse");

    assert_eq!(schema.models[0].docs, vec!["User docs".to_owned()]);
    assert_eq!(
        schema.models[0].fields[0].docs,
        vec!["Identifier docs".to_owned()]
    );
    assert_eq!(
        schema.models[0].fields[1].docs,
        vec!["Email docs".to_owned()]
    );
    assert_eq!(schema.procedures[0].docs, vec!["Feed docs".to_owned()]);
}

#[test]
fn attaches_param_docs_and_precise_spans_to_procedure_args() {
    let source = r#"
/// Feed docs
/// @param limit Maximum items to fetch
procedure getFeed(limit: Int): Int
"#;
    let schema = parse_schema(source).expect("schema with parameter docs should parse");
    let arg = &schema.procedures[0].args[0];

    assert_eq!(schema.procedures[0].docs, vec!["Feed docs".to_owned()]);
    assert_eq!(arg.docs, vec!["Maximum items to fetch".to_owned()]);
    assert_eq!(&source[arg.span.start..arg.span.end], "limit: Int");
}

#[test]
fn parses_built_in_page_return_type() {
    let source = r#"
model Post {
  id Int @id
}

procedure getFeedPage(): Page<Post>
"#;
    let schema = parse_schema(source).expect("schema with Page<T> return should parse");
    let return_type = &schema.procedures[0].return_type;

    assert_eq!(return_type.name, "Page");
    assert_eq!(return_type.generic_args.len(), 1);
    assert_eq!(return_type.generic_args[0].name, "Post");
    assert_eq!(
        &source[return_type.name_span.start..return_type.name_span.end],
        "Page"
    );
    assert_eq!(
        &source[return_type.generic_args[0].name_span.start
            ..return_type.generic_args[0].name_span.end],
        "Post"
    );
}

#[test]
fn rejects_page_return_types_outside_procedure_returns() {
    let error = parse_schema(
        r#"
type Feed {
  posts Page<Post>
}

model Post {
  id Int @id
}
"#,
    )
    .expect_err("Page<T> fields should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure return type")
    );
}

#[test]
fn rejects_page_returns_with_scalar_items() {
    let error = parse_schema(
        r#"
procedure getCounts(): Page<Int>
"#,
    )
    .expect_err("Page<T> with scalar item should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supports declared model or type items")
    );
}

#[test]
fn accepts_bare_model_paged_attribute() {
    let schema = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@paged
}
"#,
    )
    .expect("bare @@paged should parse");

    assert_eq!(schema.models[0].attributes[0].raw, "@@paged");
}

#[test]
fn rejects_invalid_model_paged_attribute_forms() {
    let error = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@paged(mode: "offset")
}
"#,
    )
    .expect_err("configured @@paged should fail validation");

    assert!(error.to_string().contains("use bare `@@paged`"));
}

#[test]
fn preserves_precise_name_spans_for_relations_and_type_references() {
    let source = r#"
model User {
  id Int @id
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#;
    let schema = parse_schema(source).expect("schema should parse");
    let post = &schema.models[1];
    let author = &post.fields[2];
    let relation =
        parse_relation_attribute(&author.attributes[0].raw).expect("relation should parse");

    assert_eq!(&source[post.name_span.start..post.name_span.end], "Post");
    assert_eq!(
        &source[author.name_span.start..author.name_span.end],
        "author"
    );
    assert_eq!(
        &source[author.ty.name_span.start..author.ty.name_span.end],
        "User"
    );
    assert_eq!(relation.fields, vec!["authorId".to_owned()]);
    assert_eq!(relation.references, vec!["id".to_owned()]);
}

#[test]
fn preserves_recursive_relation_policy_attributes() {
    let schema = parse_schema(
        r#"
auth SessionUser {
  email String
  orgSlug String
}

model Organization {
  id Int @id
  slug String
}

model User {
  id Int @id
  email String
  banned Boolean
}

model Project {
  id Int @id
  organizationId Int
  organization Organization @relation(fields:[organizationId],references:[id])
  memberships Membership[] @relation(fields:[id],references:[projectId])
}

model Membership {
  id Int @id
  projectId Int
  userId Int
  active Boolean
  blocked Boolean
  project Project @relation(fields:[projectId],references:[id])
  user User @relation(fields:[userId],references:[id])
}

model Task {
  id Int @id
  projectId Int
  project Project @relation(fields:[projectId],references:[id])

  @@deny("read", project.memberships.some.user.banned)
  @@allow("read", project.organization.slug == auth().orgSlug && project.memberships.some.user.email == auth().email)
  @@allow("delete", project.memberships.every.active)
  @@allow("create", project.memberships.none.blocked)
}
"#,
    )
    .expect("recursive policy schema should parse");

    let task = schema
        .models
        .iter()
        .find(|model| model.name == "Task")
        .expect("task model should parse");
    assert_eq!(task.attributes.len(), 4);
    assert!(
        task.attributes[0]
            .raw
            .contains("project.memberships.some.user.banned")
    );
    assert!(
        task.attributes[1]
            .raw
            .contains("project.organization.slug == auth().orgSlug")
    );
    assert!(
        task.attributes[2]
            .raw
            .contains("project.memberships.every.active")
    );
    assert!(
        task.attributes[3]
            .raw
            .contains("project.memberships.none.blocked")
    );
}

#[test]
fn tracks_field_type_span_from_token_position_not_first_substring_match() {
    let source = r#"
model Group {
  id Int @id
}

model User {
  id Int @id
  groupId Int
  GroupLabel Group @relation(fields:[groupId],references:[id])
}
"#;
    let schema = parse_schema(source).expect("schema should parse");
    let field = &schema.models[1].fields[2];

    assert_eq!(
        &source[field.name_span.start..field.name_span.end],
        "GroupLabel"
    );
    assert_eq!(
        &source[field.ty.name_span.start..field.ty.name_span.end],
        "Group"
    );
}

#[test]
fn rejects_custom_fields_on_models() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  storageKey String
  thumbnailUrl String @custom
}
"#,
    )
    .expect_err("model custom fields should fail validation");

    assert!(error.to_string().contains(
        "resolver-backed custom fields are currently only supported on `type` declarations"
    ));
}

#[test]
fn expands_mixin_fields_via_model_use_attribute() {
    let schema = parse_schema(
        r#"
mixin Timestamps {
  createdAt DateTime
  updatedAt DateTime
}

model Post {
  @use(Timestamps)
  id Int @id
  title String
}
"#,
    )
    .expect("mixin usage should parse");

    let post = &schema.models[0];
    assert_eq!(
        post.fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec!["createdAt", "updatedAt", "id", "title"]
    );
    assert!(post.attributes.is_empty());
}

#[test]
fn model_local_fields_override_mixin_fields() {
    let schema = parse_schema(
        r#"
mixin Timestamps {
  createdAt DateTime
}

model Post {
  @use(Timestamps)
  id Int @id
  createdAt DateTime?
}
"#,
    )
    .expect("model field override should parse");

    let post = &schema.models[0];
    assert_eq!(post.fields.len(), 2);
    assert_eq!(post.fields[1].name, "createdAt");
    assert_eq!(
        post.fields[1].ty.arity,
        cratestack_core::TypeArity::Optional
    );
}

#[test]
fn rejects_model_use_with_unknown_mixin() {
    let error = parse_schema(
        r#"
model Post {
  @use(UnknownMixin)
  id Int @id
}
"#,
    )
    .expect_err("unknown mixin should fail");

    assert!(error.to_string().contains("unknown mixin `UnknownMixin`"));
}

#[test]
fn rejects_mixin_id_fields() {
    let error = parse_schema(
        r#"
mixin Identity {
  id Int @id
}

model Post {
  @use(Identity)
  title String
}
"#,
    )
    .expect_err("mixin @id should fail");

    assert!(error.to_string().contains("cannot declare @id"));
}

#[test]
fn parses_version_attribute_on_int_field() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Int
  version Int @version
}
"#,
    )
    .expect("schema with @version should parse");

    let version_field = &schema.models[0].fields[2];
    assert_eq!(version_field.name, "version");
    assert!(
        version_field.attributes.iter().any(|a| a.raw == "@version"),
        "@version attribute should be present"
    );
}

#[test]
fn rejects_two_version_fields_per_model() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  v1 Int @version
  v2 Int @version
}
"#,
    )
    .expect_err("two @version fields should fail");

    assert!(
        error.to_string().contains("more than one @version"),
        "error message mentions duplicate @version: {error}",
    );
}

#[test]
fn rejects_version_on_non_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  version String @version
}
"#,
    )
    .expect_err("@version on String should fail");

    assert!(
        error.to_string().contains("must be a required `Int`"),
        "error message mentions Int requirement: {error}",
    );
}

#[test]
fn rejects_version_on_optional_int() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  version Int? @version
}
"#,
    )
    .expect_err("@version on Int? should fail");

    assert!(
        error.to_string().contains("must be a required `Int`"),
        "error message mentions Int requirement: {error}",
    );
}

#[test]
fn rejects_version_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @version
}
"#,
    )
    .expect_err("@version on @id should fail");

    assert!(
        error
            .to_string()
            .contains("must not also be the primary key"),
        "error message: {error}",
    );
}

#[test]
fn accepts_string_validators() {
    parse_schema(
        r#"
model Account {
  id Int @id
  email String @email
  name String @length(min: 1, max: 200)
  currency String @iso4217
  website String @uri
  slug String @regex("^[a-z0-9-]+$")
}
"#,
    )
    .expect("validator-decorated schema should parse");
}

#[test]
fn rejects_length_on_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  count Int @length(min: 1)
}
"#,
    )
    .expect_err("@length on Int should fail");

    assert!(
        error.to_string().contains("only valid on String or Bytes"),
        "error: {error}",
    );
}

#[test]
fn rejects_email_on_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  count Int @email
}
"#,
    )
    .expect_err("@email on Int should fail");

    assert!(
        error.to_string().contains("only valid on String"),
        "error: {error}",
    );
}

#[test]
fn rejects_invalid_regex_pattern() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  bad String @regex("[unterminated")
}
"#,
    )
    .expect_err("invalid regex should fail at parse time");

    assert!(
        error.to_string().contains("not a valid regex"),
        "error: {error}",
    );
}

#[test]
fn rejects_length_with_min_above_max() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  name String @length(min: 10, max: 5)
}
"#,
    )
    .expect_err("min > max should fail");

    assert!(
        error.to_string().contains("min (10) must be <= max"),
        "error: {error}",
    );
}

#[test]
fn rejects_range_on_string_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  name String @range(min: 0)
}
"#,
    )
    .expect_err("@range on String should fail");

    assert!(
        error.to_string().contains("only valid on Int or Decimal"),
        "error: {error}",
    );
}

#[test]
fn accepts_decimal_scalar_in_models_and_procedures() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal
  available Decimal?
}

type CreditInput {
  accountId Int
  amount Decimal
}

mutation procedure credit(args: CreditInput): Account
"#,
    )
    .expect("schema with Decimal should parse");

    let balance = &schema.models[0].fields[1];
    assert_eq!(balance.name, "balance");
    assert_eq!(balance.ty.name, "Decimal");
    let amount = &schema.types[0].fields[1];
    assert_eq!(amount.ty.name, "Decimal");
}

#[test]
fn decimal_field_can_carry_range_validator() {
    parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @range(min: 0)
}
"#,
    )
    .expect("@range on Decimal should be accepted at parse time");
}

#[test]
fn accepts_soft_delete_and_retain_attributes() {
    let schema = parse_schema(
        r#"
model Customer {
  id Int @id
  email String

  @@soft_delete
  @@retain(days: 2555)
}
"#,
    )
    .expect("model with soft-delete + retain should parse");

    let attrs = &schema.models[0].attributes;
    assert!(attrs.iter().any(|a| a.raw == "@@soft_delete"));
    assert!(attrs.iter().any(|a| a.raw == "@@retain(days: 2555)"));
}

#[test]
fn rejects_retain_without_days_argument() {
    let error = parse_schema(
        r#"
model Customer {
  id Int @id

  @@retain(weeks: 12)
}
"#,
    )
    .expect_err("@@retain(weeks: 12) should fail");

    assert!(
        error.to_string().contains("`@@retain` requires `days: N`"),
        "error: {error}",
    );
}

#[test]
fn rejects_soft_delete_with_args() {
    let error = parse_schema(
        r#"
model Customer {
  id Int @id

  @@soft_delete(column: "deleted")
}
"#,
    )
    .expect_err("@@soft_delete(...) should fail");

    assert!(
        error.to_string().contains("does not take arguments"),
        "error: {error}",
    );
}

#[test]
fn accepts_audit_attribute_on_model() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal

  @@audit
}
"#,
    )
    .expect("model with @@audit should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@audit"),
        "expected @@audit in attributes",
    );
}

#[test]
fn rejects_audit_with_arguments() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id

  @@audit(level: "full")
}
"#,
    )
    .expect_err("@@audit with args should fail");

    assert!(
        error.to_string().contains("does not take arguments"),
        "error: {error}",
    );
}

#[test]
fn accepts_readonly_and_server_only_field_attributes() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly
  internalScore Int @server_only
}
"#,
    )
    .expect("schema with field-policy attributes should parse");

    let fields = &schema.models[0].fields;
    assert!(
        fields[1].attributes.iter().any(|a| a.raw == "@readonly"),
        "expected @readonly on balance",
    );
    assert!(
        fields[2].attributes.iter().any(|a| a.raw == "@server_only"),
        "expected @server_only on internalScore",
    );
}

#[test]
fn rejects_readonly_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @readonly
}
"#,
    )
    .expect_err("@readonly on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @readonly"),
        "error: {error}",
    );
}

#[test]
fn rejects_server_only_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @server_only
}
"#,
    )
    .expect_err("@server_only on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @server_only"),
        "error: {error}",
    );
}

#[test]
fn rejects_readonly_and_server_only_together() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly @server_only
}
"#,
    )
    .expect_err("combining @readonly + @server_only should fail");

    assert!(
        error
            .to_string()
            .contains("declares both @readonly and @server_only"),
        "error: {error}",
    );
}

#[test]
fn accepts_pii_and_sensitive_field_attributes() {
    let schema = parse_schema(
        r#"
model Customer {
  id Int @id
  email String @pii
  riskScore Int @sensitive
}
"#,
    )
    .expect("schema with @pii and @sensitive should parse");

    let fields = &schema.models[0].fields;
    assert!(fields[1].attributes.iter().any(|a| a.raw == "@pii"));
    assert!(fields[2].attributes.iter().any(|a| a.raw == "@sensitive"));
}

#[test]
fn accepts_isolation_attribute_on_procedure() {
    let schema = parse_schema(
        r#"
type TransferInput {
  from Int
  to Int
}

mutation procedure transfer(args: TransferInput): TransferInput
  @isolation("serializable")
"#,
    )
    .expect("procedure with @isolation should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs
            .iter()
            .any(|a| a.raw == "@isolation(\"serializable\")"),
        "expected @isolation in attributes: {attrs:?}",
    );
}

#[test]
fn accepts_isolation_repeatable_read() {
    parse_schema(
        r#"
type Ping {
  nonce String
}

procedure read_only(args: Ping): Ping
  @isolation("repeatable_read")
"#,
    )
    .expect("repeatable_read isolation should parse");
}

#[test]
fn rejects_invalid_isolation_level() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @isolation("snapshot")
"#,
    )
    .expect_err("unknown isolation level should fail");

    assert!(
        error
            .to_string()
            .contains("unknown transaction isolation level"),
        "error: {error}",
    );
}

#[test]
fn rejects_isolation_missing_argument() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @isolation
"#,
    )
    .expect_err("@isolation without args should fail");

    assert!(
        error
            .to_string()
            .contains("@isolation requires a quoted level argument"),
        "error: {error}",
    );
}

#[test]
fn accepts_api_version_and_deprecated_on_procedure() {
    let schema = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("v1")
  @deprecated("use healthcheck_v2")
"#,
    )
    .expect("procedure with @api_version + @deprecated should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@api_version(\"v1\")"),
        "expected @api_version: {attrs:?}",
    );
    assert!(
        attrs
            .iter()
            .any(|a| a.raw == "@deprecated(\"use healthcheck_v2\")"),
        "expected @deprecated",
    );
}

#[test]
fn rejects_empty_api_version() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("")
"#,
    )
    .expect_err("empty @api_version should fail");

    assert!(
        error.to_string().contains("@api_version must not be empty"),
        "error: {error}",
    );
}

#[test]
fn rejects_api_version_with_invalid_characters() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("v 1")
"#,
    )
    .expect_err("@api_version with space should fail");

    assert!(
        error.to_string().contains("must contain only alphanumeric"),
        "error: {error}",
    );
}

#[test]
fn parses_no_idempotency_attribute_on_procedure() {
    let schema = parse_schema(
        r#"
type Ping {
  nonce String
}

mutation procedure healthcheck(args: Ping): Ping
  @no_idempotency
"#,
    )
    .expect("procedure with @no_idempotency should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@no_idempotency"),
        "procedure attributes should include @no_idempotency: {:?}",
        attrs,
    );
}

#[test]
fn transport_directive_defaults_to_rest_when_omitted() {
    let schema = parse_schema(
        r#"
model Widget {
  id Int @id
}
"#,
    )
    .expect("schema without transport directive should parse");
    assert_eq!(schema.transport, TransportStyle::Rest);
}

#[test]
fn transport_directive_selects_rpc() {
    let schema = parse_schema(
        r#"
transport rpc

model Widget {
  id Int @id
}
"#,
    )
    .expect("schema with `transport rpc` should parse");
    assert_eq!(schema.transport, TransportStyle::Rpc);
}

#[test]
fn transport_directive_selects_rest_explicitly() {
    let schema = parse_schema(
        r#"
transport rest

model Widget {
  id Int @id
}
"#,
    )
    .expect("schema with `transport rest` should parse");
    assert_eq!(schema.transport, TransportStyle::Rest);
}

#[test]
fn transport_directive_rejects_unknown_style() {
    let err = parse_schema(
        r#"
transport graphql

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("unknown transport style should be rejected");
    assert!(
        err.to_string().contains("unknown transport style"),
        "error should mention unknown transport style, got: {err}",
    );
}

#[test]
fn transport_directive_rejects_duplicate() {
    let err = parse_schema(
        r#"
transport rpc
transport rest

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("duplicate transport directive should be rejected");
    assert!(
        err.to_string().contains("duplicate"),
        "error should mention duplicate, got: {err}",
    );
}
