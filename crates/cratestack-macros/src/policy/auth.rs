use cratestack_core::{Field, TypeDecl};

// The term recognisers live in `cratestack-policy` so the contract analysis
// reads terms exactly as these dialects do.
pub(super) use cratestack_policy::{
    is_auth_is_system_term, parse_builtin_policy_call, parse_string_literal,
};

pub(crate) fn find_auth_field<'a>(
    auth: Option<&'a cratestack_core::AuthBlock>,
    types: &'a [TypeDecl],
    field: &str,
) -> Result<&'a Field, String> {
    let auth = auth.ok_or_else(|| {
        format!("read policy references auth field `{field}` but schema has no auth block")
    })?;
    if let Some(exact) = auth.fields.iter().find(|candidate| candidate.name == field) {
        return Ok(exact);
    }
    resolve_auth_field_path(auth, types, field)
}

fn resolve_auth_field_path<'a>(
    auth: &'a cratestack_core::AuthBlock,
    types: &'a [TypeDecl],
    path: &str,
) -> Result<&'a Field, String> {
    let Some((root, rest)) = path.split_once('.') else {
        return Err(format!("unknown auth field `{path}` in read policy"));
    };
    let root_field = auth
        .fields
        .iter()
        .find(|candidate| candidate.name == root)
        .ok_or_else(|| format!("unknown auth field `{path}` in read policy"))?;
    resolve_auth_type_field_path(types, &root_field.ty.name, rest, path)
}

fn resolve_auth_type_field_path<'a>(
    types: &'a [TypeDecl],
    type_name: &str,
    path: &str,
    original_path: &str,
) -> Result<&'a Field, String> {
    let ty = types
        .iter()
        .find(|candidate| candidate.name == type_name)
        .ok_or_else(|| format!("unknown auth field `{original_path}` in read policy"))?;
    let Some((head, tail)) = path.split_once('.') else {
        return ty
            .fields
            .iter()
            .find(|candidate| candidate.name == path)
            .ok_or_else(|| format!("unknown auth field `{original_path}` in read policy"));
    };
    let field = ty
        .fields
        .iter()
        .find(|candidate| candidate.name == head)
        .ok_or_else(|| format!("unknown auth field `{original_path}` in read policy"))?;
    resolve_auth_type_field_path(types, &field.ty.name, tail, original_path)
}
