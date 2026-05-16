//! Reverse mapping: `RpcListInput` → URL query string for the existing
//! list handler's parser.

use super::inputs::RpcListInput;

/// Synthesize a URL query string from an [`RpcListInput`] in exactly the
/// shape the macro-generated `parse_model_list_query` parses. Returns
/// `None` when the input has no fields set (the existing handler treats a
/// missing query the same as an empty one — no point allocating).
pub fn synthesize_list_query(input: &RpcListInput) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(limit) = input.limit {
        pairs.push(("limit".to_owned(), limit.to_string()));
    }
    if let Some(offset) = input.offset {
        pairs.push(("offset".to_owned(), offset.to_string()));
    }
    if let Some(fields) = &input.fields {
        pairs.push(("fields".to_owned(), fields.join(",")));
    }
    if let Some(include) = &input.include {
        pairs.push(("include".to_owned(), include.join(",")));
    }
    for (relation, fields) in &input.include_fields {
        pairs.push((format!("includeFields[{relation}]"), fields.join(",")));
    }
    if let Some(sort) = &input.sort {
        pairs.push(("sort".to_owned(), sort.clone()));
    }
    if let Some(where_expr) = &input.where_expr {
        pairs.push(("where".to_owned(), where_expr.clone()));
    }
    if let Some(or) = &input.or {
        pairs.push(("or".to_owned(), or.clone()));
    }
    for predicate in &input.filters {
        pairs.push((predicate.key.clone(), predicate.value.clone()));
    }

    if pairs.is_empty() {
        return None;
    }

    Some(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish(),
    )
}
