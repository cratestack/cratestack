//! Free-function emitters (`<field>_eq(value)` etc.); mirror of [`super::methods`].
use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::{ident, supports_comparison};

use super::super::RelationPathSegment;
use super::op_expr;

pub(in super::super) fn append_required_filter_functions(
    fns: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Required {
        return;
    }

    let eq_ident = ident(&format!("{}_eq", field.name));
    let ne_ident = ident(&format!("{}_ne", field.name));
    let in_ident = ident(&format!("{}_in", field.name));
    let eq = op_expr(field_type, column, quote! { eq(value) }, wrappers);
    let ne = op_expr(field_type, column, quote! { ne(value) }, wrappers);
    let in_ = op_expr(field_type, column, quote! { in_(values) }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #eq_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #eq
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #ne_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #ne
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #in_ident<I, V>(values: I) -> ::cratestack::FilterExpr
        where
            I: ::core::iter::IntoIterator<Item = V>,
            V: ::cratestack::IntoSqlValue,
        {
            #in_
        }
    });

    if !supports_comparison(field) {
        return;
    }
    let lt_ident = ident(&format!("{}_lt", field.name));
    let lte_ident = ident(&format!("{}_lte", field.name));
    let gt_ident = ident(&format!("{}_gt", field.name));
    let gte_ident = ident(&format!("{}_gte", field.name));
    let lt = op_expr(field_type, column, quote! { lt(value) }, wrappers);
    let lte = op_expr(field_type, column, quote! { lte(value) }, wrappers);
    let gt = op_expr(field_type, column, quote! { gt(value) }, wrappers);
    let gte = op_expr(field_type, column, quote! { gte(value) }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #lt_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #lt
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #lte_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #lte
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #gt_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #gt
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #gte_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #gte
        }
    });
}

pub(in super::super) fn append_boolean_filter_functions(
    fns: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "Boolean" && field.ty.arity == TypeArity::Required) {
        return;
    }
    let true_ident = ident(&format!("{}_is_true", field.name));
    let false_ident = ident(&format!("{}_is_false", field.name));
    let is_true = op_expr(field_type, column, quote! { is_true() }, wrappers);
    let is_false = op_expr(field_type, column, quote! { is_false() }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #true_ident() -> ::cratestack::FilterExpr {
            #is_true
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #false_ident() -> ::cratestack::FilterExpr {
            #is_false
        }
    });
}

pub(in super::super) fn append_required_text_filter_functions(
    fns: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(matches!(field.ty.name.as_str(), "String" | "Cuid")
        && field.ty.arity == TypeArity::Required)
    {
        return;
    }
    let contains_ident = ident(&format!("{}_contains", field.name));
    let starts_with_ident = ident(&format!("{}_starts_with", field.name));
    let contains = op_expr(field_type, column, quote! { contains(value) }, wrappers);
    let starts_with = op_expr(field_type, column, quote! { starts_with(value) }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #contains_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #starts_with_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with
        }
    });
}

pub(in super::super) fn append_optional_filter_functions(
    fns: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Optional {
        return;
    }
    let null_ident = ident(&format!("{}_is_null", field.name));
    let not_null_ident = ident(&format!("{}_is_not_null", field.name));
    let is_null = op_expr(field_type, column, quote! { is_null() }, wrappers);
    let is_not_null = op_expr(field_type, column, quote! { is_not_null() }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #null_ident() -> ::cratestack::FilterExpr {
            #is_null
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #not_null_ident() -> ::cratestack::FilterExpr {
            #is_not_null
        }
    });
}

pub(in super::super) fn append_optional_string_filter_functions(
    fns: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "String" && field.ty.arity == TypeArity::Optional) {
        return;
    }
    let contains_ident = ident(&format!("{}_contains", field.name));
    let starts_with_ident = ident(&format!("{}_starts_with", field.name));
    let contains = op_expr(field_type, column, quote! { contains(value) }, wrappers);
    let starts_with = op_expr(field_type, column, quote! { starts_with(value) }, wrappers);
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #contains_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #starts_with_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with
        }
    });
}
