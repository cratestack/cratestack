//! Placements of `@server_only` that have no effect, and are refused
//! (`validate::server_only_placement`), next to the placements that keep
//! working.

use super::parse_schema;

#[track_caller]
fn refused(source: &str, needle: &str) {
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(error.to_string().contains(needle), "error: {error}");
}

// Rule 1: a `type` field.
#[test]
fn refuses_server_only_on_a_type_field() {
    refused(
        "type Receipt {\n  total Int\n  secret String @server_only\n}\n\
         procedure latest(): Receipt\n",
        "field `Receipt.secret` declares @server_only, but `Receipt` is a `type`",
    );
}

// Rule 2: a relation field (to-one, and to-many).
#[test]
fn refuses_server_only_on_a_to_one_relation_field() {
    refused(
        "model User {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  authorId Int\n\
         author User @relation(fields: [authorId], references: [id]) @server_only\n}\n",
        "relation field `Post.author` declares @server_only",
    );
}

#[test]
fn refuses_server_only_on_a_to_many_relation_field() {
    refused(
        "model User {\n  id Int @id\n\
         posts Post[] @relation(fields: [id], references: [authorId]) @server_only\n}\n\
         model Post {\n  id Int @id\n  authorId Int\n}\n",
        "relation field `User.posts` declares @server_only",
    );
}

// Rule 3: a relation key, whichever side declares the relation.
#[test]
fn refuses_server_only_on_a_foreign_key_named_in_fields() {
    refused(
        "model User {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  authorId Int @server_only\n\
         author User @relation(fields: [authorId], references: [id])\n}\n",
        "field `Post.authorId` declares @server_only but is a key of relation `Post.author`",
    );
}

#[test]
fn refuses_server_only_on_a_foreign_key_named_only_by_the_other_side() {
    // Only the to-many side is declared: the key appears in `references`.
    refused(
        "model User {\n  id Int @id\n\
         posts Post[] @relation(fields: [id], references: [authorId])\n}\n\
         model Post {\n  id Int @id\n  authorId Int @server_only\n}\n",
        "field `Post.authorId` declares @server_only but is a key of relation `User.posts`",
    );
}

#[test]
fn refuses_server_only_on_a_non_primary_referenced_key() {
    refused(
        "model User {\n  id Int @id\n  email String @unique @server_only\n}\n\
         model Post {\n  id Int @id\n  authorEmail String\n\
         author User @relation(fields: [authorEmail], references: [email])\n}\n",
        "field `User.email` declares @server_only but is a key of relation `Post.author`",
    );
}

#[test]
fn refuses_server_only_on_a_self_relation_key() {
    refused(
        "model Node {\n  id Int @id\n  parentId Int? @server_only\n\
         parent Node? @relation(fields: [parentId], references: [id])\n}\n",
        "field `Node.parentId` declares @server_only but is a key of relation `Node.parent`",
    );
}

#[test]
fn refuses_server_only_on_a_foreign_key_from_a_mixin() {
    refused(
        "mixin Owned {\n  ownerId Int @server_only\n}\n\
         model User {\n  id Int @id\n}\n\
         model Doc {\n  @use(Owned)\n  id Int @id\n\
         owner User @relation(fields: [ownerId], references: [id])\n}\n",
        "field `Doc.ownerId` declares @server_only but is a key of relation `Doc.owner`",
    );
}

// Rule 4: together with `@version`.
#[test]
fn refuses_server_only_with_version() {
    refused(
        "model Account {\n  id Int @id\n  rev Int @version @server_only\n}\n",
        "field `Account.rev` declares both @version and @server_only",
    );
}

// Rule 5: a field of the `auth` block. The diagnostic spans the attribute.
#[test]
fn refuses_server_only_on_an_auth_field() {
    let source = "auth Ctx {\n  id Int\n  tenant String @server_only\n}\n\
                  model A {\n  id Int @id\n}\n";
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(
        error.to_string().contains(
            "field `Ctx.tenant` declares @server_only, but `Ctx` is the `auth` block, not a \
             `model`"
        ),
        "error: {error}"
    );
    assert_eq!(&source[error.span()], "@server_only", "{error}");
    parse_schema("auth Ctx {\n  id Int\n  tenant String\n}\nmodel A {\n  id Int @id\n}\n")
        .expect("the same auth field without @server_only stays accepted");
}

// Positive controls: the supported placement, and the neighbours of each
// refused one without the attribute.
#[test]
fn accepts_server_only_on_a_plain_model_scalar_next_to_a_relation() {
    parse_schema(
        "model User {\n  id Int @id\n  passwordHash String @server_only\n\
         posts Post[] @relation(fields: [id], references: [authorId])\n}\n\
         model Post {\n  id Int @id\n  rev Int @version\n  note String @server_only\n\
         authorId Int\n  author User @relation(fields: [authorId], references: [id])\n}\n\
         type Receipt {\n  total Int\n}\n",
    )
    .expect("a @server_only scalar that is not a relation key stays accepted");
}
