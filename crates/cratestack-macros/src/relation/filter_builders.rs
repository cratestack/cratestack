use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::{ident, supports_comparison};

use super::{RelationPathSegment, wrap_filter_expr_tokens};

pub(super) fn append_required_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Required {
        return;
    }

    let eq_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).eq(value)) },
        wrappers,
    );
    let ne_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).ne(value)) },
        wrappers,
    );
    let in_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).in_(values)) },
        wrappers,
    );
    methods.push(quote! {
        pub fn eq<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #eq_expr
        }
    });
    methods.push(quote! {
        pub fn ne<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #ne_expr
        }
    });
    methods.push(quote! {
        pub fn in_<I, V>(self, values: I) -> ::cratestack::FilterExpr
        where
            I: ::core::iter::IntoIterator<Item = V>,
            V: ::cratestack::IntoSqlValue,
        {
            #in_expr
        }
    });

    if !supports_comparison(field) {
        return;
    }
    let lt_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).lt(value)) },
        wrappers,
    );
    let lte_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).lte(value)) },
        wrappers,
    );
    let gt_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).gt(value)) },
        wrappers,
    );
    let gte_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).gte(value)) },
        wrappers,
    );
    methods.push(quote! {
        pub fn lt<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #lt_expr
        }
    });
    methods.push(quote! {
        pub fn lte<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #lte_expr
        }
    });
    methods.push(quote! {
        pub fn gt<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #gt_expr
        }
    });
    methods.push(quote! {
        pub fn gte<V: ::cratestack::IntoSqlValue>(self, value: V) -> ::cratestack::FilterExpr {
            #gte_expr
        }
    });
}

pub(super) fn append_boolean_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "Boolean" && field.ty.arity == TypeArity::Required) {
        return;
    }
    let true_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_true()) },
        wrappers,
    );
    let false_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_false()) },
        wrappers,
    );
    methods.push(quote! {
        pub fn is_true(self) -> ::cratestack::FilterExpr {
            #true_expr
        }
    });
    methods.push(quote! {
        pub fn is_false(self) -> ::cratestack::FilterExpr {
            #false_expr
        }
    });
}

pub(super) fn append_required_text_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
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
    let contains_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).contains(value)) },
        wrappers,
    );
    let starts_with_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).starts_with(value)) },
        wrappers,
    );
    methods.push(quote! {
        pub fn contains(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains_expr
        }
    });
    methods.push(quote! {
        pub fn starts_with(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with_expr
        }
    });
}

pub(super) fn append_optional_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if field.ty.arity != TypeArity::Optional {
        return;
    }
    let null_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_null()) },
        wrappers,
    );
    let not_null_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_not_null()) },
        wrappers,
    );
    methods.push(quote! {
        pub fn is_null(self) -> ::cratestack::FilterExpr {
            #null_expr
        }
    });
    methods.push(quote! {
        pub fn is_not_null(self) -> ::cratestack::FilterExpr {
            #not_null_expr
        }
    });
}

pub(super) fn append_optional_string_builder_methods(
    methods: &mut Vec<proc_macro2::TokenStream>,
    field: &Field,
    wrappers: &[RelationPathSegment],
    field_type: &proc_macro2::TokenStream,
    column: &str,
) {
    if !(field.ty.name == "String" && field.ty.arity == TypeArity::Optional) {
        return;
    }
    let contains_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).contains(value)) },
        wrappers,
    );
    let starts_with_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).starts_with(value)) },
        wrappers,
    );
    methods.push(quote! {
        pub fn contains(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains_expr
        }
    });
    methods.push(quote! {
        pub fn starts_with(self, value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with_expr
        }
    });
}

pub(super) fn append_required_filter_functions(
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
    let eq_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).eq(value)) },
        wrappers,
    );
    let ne_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).ne(value)) },
        wrappers,
    );
    let in_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).in_(values)) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #eq_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #eq_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #ne_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #ne_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #in_ident<I, V>(values: I) -> ::cratestack::FilterExpr
        where
            I: ::core::iter::IntoIterator<Item = V>,
            V: ::cratestack::IntoSqlValue,
        {
            #in_expr
        }
    });

    if !supports_comparison(field) {
        return;
    }
    let lt_ident = ident(&format!("{}_lt", field.name));
    let lte_ident = ident(&format!("{}_lte", field.name));
    let gt_ident = ident(&format!("{}_gt", field.name));
    let gte_ident = ident(&format!("{}_gte", field.name));
    let lt_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).lt(value)) },
        wrappers,
    );
    let lte_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).lte(value)) },
        wrappers,
    );
    let gt_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).gt(value)) },
        wrappers,
    );
    let gte_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).gte(value)) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #lt_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #lt_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #lte_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #lte_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #gt_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #gt_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #gte_ident<V: ::cratestack::IntoSqlValue>(value: V) -> ::cratestack::FilterExpr {
            #gte_expr
        }
    });
}

pub(super) fn append_boolean_filter_functions(
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
    let true_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_true()) },
        wrappers,
    );
    let false_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_false()) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #true_ident() -> ::cratestack::FilterExpr {
            #true_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #false_ident() -> ::cratestack::FilterExpr {
            #false_expr
        }
    });
}

pub(super) fn append_required_text_filter_functions(
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
    let contains_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).contains(value)) },
        wrappers,
    );
    let starts_with_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).starts_with(value)) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #contains_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #starts_with_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with_expr
        }
    });
}

pub(super) fn append_optional_filter_functions(
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
    let null_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_null()) },
        wrappers,
    );
    let not_null_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).is_not_null()) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #null_ident() -> ::cratestack::FilterExpr {
            #null_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #not_null_ident() -> ::cratestack::FilterExpr {
            #not_null_expr
        }
    });
}

pub(super) fn append_optional_string_filter_functions(
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
    let contains_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).contains(value)) },
        wrappers,
    );
    let starts_with_expr = wrap_filter_expr_tokens(
        quote! { ::cratestack::FilterExpr::from(::cratestack::FieldRef::<(), #field_type>::new(#column).starts_with(value)) },
        wrappers,
    );
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #contains_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #contains_expr
        }
    });
    fns.push(quote! {
        #[allow(non_snake_case)]
        pub fn #starts_with_ident(value: impl ::core::convert::Into<String>) -> ::cratestack::FilterExpr {
            #starts_with_expr
        }
    });
}
