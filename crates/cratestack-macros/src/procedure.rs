use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeArity, TypeDecl, TypeRef};
use quote::quote;

use crate::policy::{
    generate_procedure_policy, parse_procedure_allow_expression, parse_procedure_deny_expression,
};
use crate::shared::{doc_attrs, ident, is_primary_key, to_snake_case, value_tokens};

pub(crate) fn generate_procedure_module(
    procedure: &Procedure,
    models: &[Model],
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&procedure.name));
    let docs = doc_attrs(&procedure.docs);
    let mut allow_expressions = Vec::new();
    let mut deny_expressions = Vec::new();
    let mut model_authorizers = Vec::new();
    for attribute in &procedure.attributes {
        if let Some(expression) = parse_procedure_allow_expression(&attribute.raw) {
            allow_expressions.push(expression?);
        }
        if let Some(expression) = parse_procedure_deny_expression(&attribute.raw) {
            deny_expressions.push(expression?);
        }
        if let Some(authorizer) = parse_procedure_model_authorizer(&attribute.raw) {
            model_authorizers.push(generate_procedure_model_authorizer(
                authorizer?,
                procedure,
                models,
                types,
            )?);
        }
    }
    let allow_policies = allow_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let deny_policies = deny_expressions
        .into_iter()
        .map(|expression| generate_procedure_policy(expression, procedure, types, auth))
        .collect::<Result<Vec<_>, _>>()?;
    let procedure_name = &procedure.name;
    let args_struct = generate_procedure_args_struct(procedure, types, enum_names);
    let output_type = procedure_output_tokens(&procedure.return_type, types, enum_names);

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #procedure_name;
            pub const ALLOW_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#allow_policies),*];
            pub const DENY_POLICIES: &[::cratestack::ProcedurePolicy] = &[#(#deny_policies),*];

            #args_struct

            pub type Output = #output_type;

            pub fn authorize<A: ::cratestack::ProcedureArgs + ?Sized>(
                args: &A,
                ctx: &::cratestack::CoolContext,
            ) -> Result<(), ::cratestack::CoolError> {
                let started = ::std::time::Instant::now();
                let result = ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx);
                match &result {
                    Ok(()) => ::cratestack::tracing::debug!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "authorize",
                        cratestack_authenticated = ctx.is_authenticated(),
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure authorized",
                    ),
                    Err(error) => ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "authorize",
                        cratestack_authenticated = ctx.is_authenticated(),
                        cratestack_error = error.code(),
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure authorization failed",
                    ),
                }
                result
            }

            pub async fn authorize_with_db(
                db: &super::super::Cratestack,
                args: &Args,
                ctx: &::cratestack::CoolContext,
            ) -> Result<(), ::cratestack::CoolError> {
                let started = ::std::time::Instant::now();
                ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx)?;
                #(#model_authorizers)*
                ::cratestack::tracing::debug!(
                    target: "cratestack",
                    cratestack_procedure = NAME,
                    cratestack_operation = "authorize_with_db",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    "cratestack procedure db authorization completed",
                );
                Ok(())
            }

            pub async fn invoke<A, F, Fut, T>(
                args: &A,
                ctx: &::cratestack::CoolContext,
                f: F,
            ) -> Result<T, ::cratestack::CoolError>
            where
                A: ::cratestack::ProcedureArgs + ?Sized,
                F: FnOnce() -> Fut,
                Fut: ::core::future::Future<Output = Result<T, ::cratestack::CoolError>>,
            {
                let span = ::cratestack::tracing::info_span!(
                    "cratestack_procedure_invoke",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke",
                    cratestack_authenticated = ctx.is_authenticated(),
                );
                let _guard = span.enter();
                let started = ::std::time::Instant::now();
                ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, args, ctx)?;
                let result = f().await;
                match &result {
                    Ok(_) => ::cratestack::tracing::info!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "invoke",
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure completed",
                    ),
                    Err(error) => ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "invoke",
                        cratestack_error = error.code(),
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure failed",
                    ),
                }
                result
            }

            pub async fn invoke_with_db<F, Fut, T>(
                db: &super::super::Cratestack,
                args: &Args,
                ctx: &::cratestack::CoolContext,
                f: F,
            ) -> Result<T, ::cratestack::CoolError>
            where
                F: FnOnce() -> Fut,
                Fut: ::core::future::Future<Output = Result<T, ::cratestack::CoolError>>,
            {
                let span = ::cratestack::tracing::info_span!(
                    "cratestack_procedure_invoke_with_db",
                    cratestack_procedure = NAME,
                    cratestack_operation = "invoke_with_db",
                    cratestack_authenticated = ctx.is_authenticated(),
                );
                let _guard = span.enter();
                let started = ::std::time::Instant::now();
                authorize_with_db(db, args, ctx).await?;
                let result = f().await;
                match &result {
                    Ok(_) => ::cratestack::tracing::info!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "invoke_with_db",
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure completed",
                    ),
                    Err(error) => ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_procedure = NAME,
                        cratestack_operation = "invoke_with_db",
                        cratestack_error = error.code(),
                        cratestack_duration_ms = started.elapsed().as_millis() as u64,
                        "cratestack procedure failed",
                    ),
                }
                result
            }
        }
    })
}

pub(crate) fn generate_client_procedure_module(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&procedure.name));
    let docs = doc_attrs(&procedure.docs);
    let procedure_name = &procedure.name;
    let args_struct = generate_client_procedure_args_struct(procedure, types, enum_names);
    let output_type = procedure_output_tokens(&procedure.return_type, types, enum_names);

    Ok(quote! {
        #docs
        pub mod #module_ident {
            pub const NAME: &str = #procedure_name;

            #args_struct

            pub type Output = #output_type;
        }
    })
}

struct ProcedureModelAuthorizer<'a> {
    model_name: &'a str,
    action: &'a str,
    id_path: &'a str,
}

fn parse_procedure_model_authorizer(
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

fn generate_procedure_model_authorizer(
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

fn generate_procedure_args_struct(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let args_ident = ident("Args");
    let definitions = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_type = procedure_type_tokens(&arg.ty, types, enum_names);
        let docs = doc_attrs(&arg.docs);
        quote! {
            #docs
            pub #field_ident: #field_type,
        }
    });
    let value_matches = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_name = &arg.name;
        let value = value_tokens(quote! { self.#field_ident.clone() }, &arg.ty, enum_names);
        quote! { #field_name => Some(#value), }
    });
    let nested_arg_match = procedure
        .args
        .iter()
        .find(|arg| arg.name == "args")
        .and_then(|arg| types.iter().find(|candidate| candidate.name == arg.ty.name))
        .map(|_| {
            quote! {
                _ if field.starts_with("args.") => self.args.procedure_arg_value(&field[5..]),
                _ => self.args.procedure_arg_value(field),
            }
        })
        .unwrap_or_else(|| {
            quote! {
                _ => None,
            }
        });

    let default_derive = if procedure.args.is_empty() {
        quote! { , Default }
    } else {
        quote! {}
    };

    quote! {
        #[doc = "Generated argument payload for this procedure."]
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize #default_derive)]
        pub struct #args_ident {
            #(#definitions)*
        }

        impl ::cratestack::ProcedureArgs for #args_ident {
            fn procedure_arg_value(&self, field: &str) -> Option<::cratestack::Value> {
                match field {
                    #(#value_matches)*
                    #nested_arg_match
                }
            }
        }
    }
}

fn generate_client_procedure_args_struct(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let args_ident = ident("Args");
    let definitions = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_type = procedure_type_tokens(&arg.ty, types, enum_names);
        let docs = doc_attrs(&arg.docs);
        quote! {
            #docs
            pub #field_ident: #field_type,
        }
    });

    let default_derive = if procedure.args.is_empty() {
        quote! { , Default }
    } else {
        quote! {}
    };

    quote! {
        #[doc = "Generated argument payload for this procedure."]
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize #default_derive)]
        pub struct #args_ident {
            #(#definitions)*
        }
    }
}

fn procedure_output_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    procedure_type_tokens(type_ref, types, enum_names)
}

pub(crate) fn procedure_client_output_item_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Json" => quote! { ::cratestack::sqlx::types::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let model_ident = ident(other);
            quote! { super::#model_ident }
        }
    }
}

fn procedure_type_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = procedure_type_tokens(item, types, enum_names);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Json" => quote! { ::cratestack::sqlx::types::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let item_ident = ident(other);
            if types.iter().any(|ty| ty.name == other) || enum_names.contains(other) {
                quote! { super::super::types::#item_ident }
            } else {
                quote! { super::super::#item_ident }
            }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

pub(crate) fn generate_procedure_registry_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));

    Ok(quote! {
        fn #method_ident(
            &self,
            db: &super::Cratestack,
            ctx: &::cratestack::CoolContext,
            args: #module_ident::Args,
        ) -> impl ::core::future::Future<Output = Result<#module_ident::Output, ::cratestack::CoolError>> + Send;
    })
}
