//! The schema `tests_types` and `tests_models` read: models and types that
//! hold validators directly, through nesting and through cycles, and the
//! procedures that take them.

use super::Validating;

pub(super) const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model User {
  id Int @id
  name String @length(min: 3)
  bio String? @length(max: 20)
  secret String @server_only @length(min: 8)
  posts Post[] @relation(fields:[id],references:[authorId])

  @@allow("read", auth() != null)
}

// A `@computed` field makes a model unusable as a procedure argument (the
// parser refuses it), so this one is only ever checked for what its impl
// would name.
model Shouter {
  id Int @id
  word String @length(min: 1)
  shout String @computed

  @@allow("read", auth() != null)
}

model Post {
  id Int @id
  authorId Int
  title String @length(min: 1)
  author User @relation(fields:[authorId],references:[id])

  @@allow("read", auth() != null)
}

// The only validator is on a `@server_only` field, which a client can never
// fill: nothing to validate.
model Vault {
  id Int @id
  secret String @server_only @length(min: 8)

  @@allow("read", auth() != null)
}

model Bare {
  id Int @id
  note String

  @@allow("read", auth() != null)
}

type Tag {
  label String @length(min: 2)
}

type Owner {
  tags Tag[]
  backup Tag?
}

type Account {
  owner Owner
}

type Wrap {
  owner User
}

type Plain {
  note String
}

// A list cycle compiles (`Vec<Node>` has a size) and holds a validator: the
// fixpoint must terminate, and validation must recurse into the children.
type Node {
  label String @length(min: 1)
  children Node[]
}

// A cycle through two types, the validator on one side only.
type Left {
  rights Right[]
}

type Right {
  lefts Left[]
  name String @length(min: 1)
}

// Declared outermost first, so a single pass over the declarations in this
// order would not see that `Outer` is validating: only the fixpoint does.
type Outer {
  middle Middle
}

type Middle {
  inner Inner
}

type Inner {
  code String @length(min: 2)
}

// A cycle with no validator anywhere: not validating.
type Loop {
  next Loop[]
}

type Reply {
  ok Boolean
}

procedure open(args: Account): Reply
  @allow(true)

procedure takeUser(args: User): Reply
  @allow(true)

procedure takeWrap(args: Wrap): Reply
  @allow(true)

procedure takeVault(args: Vault): Reply
  @allow(true)

// The cycles hold validators, so a client has to be able to send them.
procedure walk(args: Node): Reply
  @allow(true)

procedure meet(args: Left): Reply
  @allow(true)

procedure deep(args: Outer): Reply
  @allow(true)

procedure echo(args: Plain, count: Int): Reply
  @allow(true)

procedure many(id: String, tags: Tag[], extra: Tag?, owners: User[]): Reply
  @allow(true)
"#;

pub(super) fn schema() -> cratestack_core::Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

pub(super) fn validating(schema: &cratestack_core::Schema) -> Validating {
    Validating::of(&schema.types, &schema.models)
}

pub(super) fn procedure<'a>(
    schema: &'a cratestack_core::Schema,
    name: &str,
) -> &'a cratestack_core::Procedure {
    schema
        .procedures
        .iter()
        .find(|p| p.name == name)
        .expect("procedure")
}
