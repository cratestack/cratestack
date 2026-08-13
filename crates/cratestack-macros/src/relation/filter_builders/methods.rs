//! Method emitters for filter *builders* (chained off a relation
//! field, `self`-consuming). Each function appends per-field helpers
//! when its arity/type predicate matches; non-matching fields produce
//! no methods.

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::supports_comparison;

use super::op_expr;

pub(crate) fn append_required_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Required {
        return;
    }

    let eq = op_expr(field_type, column, quote! { eq(value) });
    let ne = op_expr(field_type, column, quote! { ne(value) });
    let in_ = op_expr(field_type, column, quote! { in_(values) });
    methods.push(quote! {
        pub fn eq<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #eq
        }
    });
    methods.push(quote! {
        pub fn ne<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #ne
        }
    });
    methods.push(quote! {
        pub fn in_<I, V>(self, values: I) -> ::cratestack::FilterExpr
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
    let lt = op_expr(field_type, column, quote! { lt(value) });
    let lte = op_expr(field_type, column, quote! { lte(value) });
    let gt = op_expr(field_type, column, quote! { gt(value) });
    let gte = op_expr(field_type, column, quote! { gte(value) });
    methods.push(quote! {
        pub fn lt<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #lt
        }
    });
    methods.push(quote! {
        pub fn lte<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #lte
        }
    });
    methods.push(quote! {
        pub fn gt<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #gt
        }
    });
    methods.push(quote! {
        pub fn gte<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #gte
        }
    });
}

pub(crate) fn append_boolean_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "Boolean" && field.ty.arity == TypeArity::Required) {
        return;
    }
    let is_true = op_expr(field_type, column, quote! { is_true() });
    let is_false = op_expr(field_type, column, quote! { is_false() });
    methods.push(quote! {
        pub fn is_true(self) -> ::cratestack::FilterExpr {
            #is_true
        }
    });
    methods.push(quote! {
        pub fn is_false(self) -> ::cratestack::FilterExpr {
            #is_false
        }
    });
}

pub(crate) fn append_required_text_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(matches!(field.ty.name.as_str(), "String" | "Cuid")
        && field.ty.arity == TypeArity::Required)
    {
        return;
    }
    let contains = op_expr(field_type, column, quote! { contains(value) });
    let starts_with = op_expr(field_type, column, quote! { starts_with(value) });
    methods.push(quote! {
        pub fn contains(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains
        }
    });
    methods.push(quote! {
        pub fn starts_with(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with
        }
    });
}

pub(crate) fn append_optional_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Optional {
        return;
    }
    let is_null = op_expr(field_type, column, quote! { is_null() });
    let is_not_null = op_expr(field_type, column, quote! { is_not_null() });
    methods.push(quote! {
        pub fn is_null(self) -> ::cratestack::FilterExpr {
            #is_null
        }
    });
    methods.push(quote! {
        pub fn is_not_null(self) -> ::cratestack::FilterExpr {
            #is_not_null
        }
    });
}

pub(crate) fn append_optional_string_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "String" && field.ty.arity == TypeArity::Optional) {
        return;
    }
    let contains = op_expr(field_type, column, quote! { contains(value) });
    let starts_with = op_expr(field_type, column, quote! { starts_with(value) });
    methods.push(quote! {
        pub fn contains(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains
        }
    });
    methods.push(quote! {
        pub fn starts_with(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with
        }
    });
}
