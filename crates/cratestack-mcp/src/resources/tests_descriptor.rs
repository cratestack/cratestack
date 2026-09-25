//! [`ResourceDescriptor::name`] is the URI's host, `cratestack://<name>/...`
//! (maintainer decision on #1040, which renamed the field from `schema`:
//! it holds the `mcp { name }` value, not anything about a schema file).
//! Two servers may share a segment, so the name is what tells their
//! resources apart, both in the URI a listing offers and in the one a read
//! is matched against.

use cratestack_core::{OpDescriptor, OpKind};

use super::ResourceDescriptor;
use super::uri::{Target, parse};

static OP: OpDescriptor = OpDescriptor {
    op_id: "model.Post.get",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: true,
    auth_required: false,
};

static TABLE: [ResourceDescriptor; 2] = [
    ResourceDescriptor::new("blog", "posts", 200, &OP, &OP),
    ResourceDescriptor::new("shop", "posts", 20, &OP, &OP),
];

#[test]
fn the_name_is_the_uris_host() {
    assert_eq!(TABLE[0].name, "blog");
    assert_eq!(TABLE[1].collection_uri(), "cratestack://shop/posts");
}

#[test]
fn a_shared_segment_is_told_apart_by_the_name() {
    for name in ["blog", "shop"] {
        match parse(&format!("cratestack://{name}/posts/1"), &TABLE) {
            Ok(Target::Record { resource, id }) => {
                assert_eq!((resource.name, id.as_str()), (name, "1"));
            }
            other => panic!("{name}: expected a record, got {other:?}"),
        }
        match parse(&format!("cratestack://{name}/posts"), &TABLE) {
            Ok(Target::Page { resource, .. }) => assert_eq!(resource.name, name),
            other => panic!("{name}: expected a page, got {other:?}"),
        }
    }
}
