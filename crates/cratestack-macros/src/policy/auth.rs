use cratestack_core::{Field, TypeDecl};

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

pub(super) fn parse_string_literal(value: &str) -> Option<&str> {
    if let Some(value) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return Some(value);
    }
    value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
}

pub(super) fn parse_builtin_policy_call(term: &str) -> Option<Result<(&str, &str), String>> {
    let open = term.find('(')?;
    let name = term[..open].trim();
    let args = term[open + 1..].strip_suffix(')')?.trim();
    if name.is_empty() {
        return Some(Err(format!("invalid policy function `{term}`")));
    }
    if args.is_empty() {
        return Some(Err(format!(
            "policy function `{name}` requires a single string literal argument"
        )));
    }
    if args.contains(',') {
        return Some(Err(format!(
            "policy function `{name}` currently supports exactly one string literal argument"
        )));
    }
    let Some(value) = parse_string_literal(args) else {
        return Some(Err(format!(
            "policy function `{name}` requires a string literal argument"
        )));
    };
    Some(Ok((name, value)))
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
