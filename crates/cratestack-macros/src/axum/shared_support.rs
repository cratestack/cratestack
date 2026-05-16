//! Schema-independent helper-fn tokens emitted once per module
//! (parse_model_list_query / parse_model_fetch_query / CSV parsers).
//! Spliced into `pub mod axum { ... }` exactly once.

use quote::quote;

pub(crate) fn generate_axum_shared_support() -> proc_macro2::TokenStream {
    quote! {
        fn parse_model_list_query(raw_query: Option<&str>) -> Result<ModelListQuery, CoolError> {
            let mut query = ModelListQuery::default();
            for (key, value) in ::cratestack::parse_query_pairs(raw_query)? {
                match key.as_str() {
                    "limit" => {
                        query.limit = Some(value.parse::<i64>().map_err(|error| {
                            CoolError::BadRequest(format!("invalid value '{}' for limit: {error}", value))
                        })?);
                    }
                    "offset" => {
                        query.offset = Some(value.parse::<i64>().map_err(|error| {
                            CoolError::BadRequest(format!("invalid value '{}' for offset: {error}", value))
                        })?);
                    }
                    "fields" => {
                        query.selection.fields = Some(parse_csv_query_parameter("fields", &value)?);
                    }
                    "include" => {
                        query.selection.includes = parse_csv_query_parameter("include", &value)?;
                    }
                    key if key.starts_with("includeFields[") && key.ends_with(']') => {
                        let include = parse_include_fields_parameter_name(&key)?;
                        let fields = parse_csv_query_parameter(&key, &value)?;
                        if query.selection.include_fields.insert(include.to_owned(), fields).is_some() {
                            return Err(CoolError::BadRequest(format!(
                                "{} must not be provided more than once",
                                key,
                            )));
                        }
                    }
                    "sort" => {
                        query.sort = Some(value);
                    }
                    "orderBy" => {
                        if query.sort.is_some() {
                            return Err(CoolError::BadRequest(
                                "sort and orderBy cannot both be provided".to_owned(),
                            ));
                        }
                        query.sort = Some(value);
                    }
                    "or" => {
                        query.filters.push(::cratestack::QueryExpr::Any(parse_or_group(&value)?));
                    }
                    "where" => {
                        query.filters.push(::cratestack::parse_filter_expression(&value)?);
                    }
                    _ => query.filters.push(::cratestack::QueryExpr::Predicate { key, value }),
                }
            }
            Ok(query)
        }

        fn parse_model_fetch_query(raw_query: Option<&str>) -> Result<ModelFetchQuery, CoolError> {
            let mut query = ModelFetchQuery::default();
            for (key, value) in ::cratestack::parse_query_pairs(raw_query)? {
                match key.as_str() {
                    "fields" => {
                        query.selection.fields = Some(parse_csv_query_parameter("fields", &value)?);
                    }
                    "include" => {
                        query.selection.includes = parse_csv_query_parameter("include", &value)?;
                    }
                    key if key.starts_with("includeFields[") && key.ends_with(']') => {
                        let include = parse_include_fields_parameter_name(&key)?;
                        let fields = parse_csv_query_parameter(&key, &value)?;
                        if query.selection.include_fields.insert(include.to_owned(), fields).is_some() {
                            return Err(CoolError::BadRequest(format!(
                                "{} must not be provided more than once",
                                key,
                            )));
                        }
                    }
                    unexpected => {
                        return Err(CoolError::BadRequest(format!(
                            "unsupported query parameter '{}' for fetch route",
                            unexpected,
                        )));
                    }
                }
            }
            Ok(query)
        }

        fn parse_csv_query_parameter(parameter: &str, value: &str) -> Result<Vec<String>, CoolError> {
            let selections = value
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if selections.is_empty() || selections.iter().any(|selection| selection.is_empty()) {
                return Err(CoolError::BadRequest(format!(
                    "{} must not contain empty selections",
                    parameter,
                )));
            }
            Ok(selections)
        }

        fn parse_include_fields_parameter_name(parameter: &str) -> Result<&str, CoolError> {
            let include = parameter
                .strip_prefix("includeFields[")
                .and_then(|value| value.strip_suffix(']'))
                .ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "invalid includeFields parameter '{}': expected includeFields[relation]",
                        parameter,
                    ))
                })?;
            if include.trim().is_empty() {
                return Err(CoolError::BadRequest(
                    "includeFields[relation] must target a relation name".to_owned(),
                ));
            }
            Ok(include)
        }

        fn parse_or_group(value: &str) -> Result<Vec<::cratestack::QueryExpr>, CoolError> {
            let mut filters = Vec::new();
            for raw_filter in value.split('|') {
                let raw_filter = raw_filter.trim();
                if raw_filter.is_empty() {
                    return Err(CoolError::BadRequest(
                        "or groups must not contain empty filters".to_owned(),
                    ));
                }
                let (key, value) = raw_filter.split_once('=').ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "invalid or filter '{}': expected key=value",
                        raw_filter,
                    ))
                })?;
                filters.push(::cratestack::QueryExpr::Predicate {
                    key: key.to_owned(),
                    value: value.to_owned(),
                });
            }
            if filters.is_empty() {
                return Err(CoolError::BadRequest(
                    "or groups must include at least one filter".to_owned(),
                ));
            }
            Ok(filters)
        }
    }
}
