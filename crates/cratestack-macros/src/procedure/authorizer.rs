//! `@authorize(Model, action, args.path)` attribute parsing + token
//! generation. Each authorizer becomes a `db.<model>().authorize_*`
//! call spliced into the generated `authorize_with_db`.

use cratestack_core::{Model, Procedure, TypeDecl, TypeRef};
use quote::quote;

use crate::shared::{ident, is_primary_key, to_snake_case};

pub(super) struct ProcedureModelAuthorizer<'a> {
    pub(super) model_name: &'a str,
    pub(super) action: &'a str,
    pub(super) id_path: &'a str,
}

pub(super) fn parse_procedure_model_authorizer(
    raw: &str,
) -> Option<Result<ProcedureModelAuthorizer<'_>, String>> {
    let inner = raw
        .trim()
        .strip_prefix("@authorize(")?
        .strip_suffix(')')?
        .trim();
    let parts = inner.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() != 3 {
        return Some(Err(format!(
            "invalid @authorize attribute: expected @authorize(Model, action, args.path), got `{raw}`"
        )));
    }
    Some(Ok(ProcedureModelAuthorizer {
        model_name: parts[0],
        action: parts[1].trim_matches('"').trim_matches('\''),
        id_path: parts[2],
    }))
}

pub(super) fn generate_procedure_model_authorizer(
    authorizer: ProcedureModelAuthorizer<'_>,
    procedure: &Procedure,
    models: &[Model],
    types: &[TypeDecl],
) -> Result<proc_macro2::TokenStream, String> {
    let model = models
        .iter()
        .find(|candidate| candidate.name == authorizer.model_name)
        .ok_or_else(|| {
            format!(
                "unknown model `{}` in @authorize for `{}`",
                authorizer.model_name, procedure.name
            )
        })?;
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model `{}` is missing a primary key", model.name))?;
    let id_field = resolve_procedure_path_type(procedure, types, authorizer.id_path)?;
    if id_field.name != primary_key.ty.name || id_field.arity != primary_key.ty.arity {
        return Err(format!(
            "@authorize path `{}` on `{}` must match `{}` primary key type",
            authorizer.id_path, procedure.name, model.name
        ));
    }

    let accessor_ident = ident(&to_snake_case(&model.name));
    let id_expr = procedure_path_tokens(authorizer.id_path)?;
    let check = match authorizer.action {
        "detail" | "read" => {
            quote! { db.#accessor_ident().authorize_detail(#id_expr, ctx).await?; }
        }
        "update" => quote! { db.#accessor_ident().authorize_update(#id_expr, ctx).await?; },
        "delete" => quote! { db.#accessor_ident().authorize_delete(#id_expr, ctx).await?; },
        other => {
            return Err(format!(
                "@authorize on `{}` only supports detail/read, update, and delete actions; got `{other}`",
                procedure.name
            ));
        }
    };
    Ok(check)
}

fn procedure_path_tokens(path: &str) -> Result<proc_macro2::TokenStream, String> {
    let mut segments = path.split('.');
    let Some(first) = segments.next() else {
        return Err("empty procedure path in @authorize".to_owned());
    };
    let first_ident = ident(first);
    let mut tokens = quote! { args.#first_ident.clone() };
    for segment in segments {
        let ident = ident(segment);
        tokens = quote! { #tokens.#ident.clone() };
    }
    Ok(tokens)
}

fn resolve_procedure_path_type<'a>(
    procedure: &'a Procedure,
    types: &'a [TypeDecl],
    path: &str,
) -> Result<&'a TypeRef, String> {
    let mut segments = path.split('.');
    let first = segments
        .next()
        .ok_or_else(|| format!("empty procedure path `{path}`"))?;
    let mut current = procedure
        .args
        .iter()
        .find(|arg| arg.name == first)
        .map(|arg| &arg.ty)
        .ok_or_else(|| {
            format!(
                "unknown procedure input field `{path}` on `{}`",
                procedure.name
            )
        })?;
    for segment in segments {
        let ty = types
            .iter()
            .find(|candidate| candidate.name == current.name)
            .ok_or_else(|| {
                format!(
                    "unknown procedure input field `{path}` on `{}`",
                    procedure.name
                )
            })?;
        let field = ty
            .fields
            .iter()
            .find(|candidate| candidate.name == segment)
            .ok_or_else(|| {
                format!(
                    "unknown procedure input field `{path}` on `{}`",
                    procedure.name
                )
            })?;
        current = &field.ty;
    }
    Ok(current)
}
