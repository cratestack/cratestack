//! The elements of an `in [...]` list. The macros turn each element into a
//! typed literal; the analysis only needs to know the list is one the macros
//! accept, so both split it here, with the macros' own splitter.
//!
//! A comma inside a `"…"` / `'…'` literal does not split. There is no escape
//! handling: a backslash is an ordinary character, so `["q\", "x"]` reads the
//! quote after the backslash as the end of the literal. That is how the macros
//! have always read it.

/// Split the bracket contents on commas, ignoring commas inside a
/// quoted string literal so `["a,b", "c"]` is two elements, not three.
///
/// An empty list is rejected rather than lowered to a constant `FALSE`:
/// it is far more likely a truncated edit than an intentional
/// "match nothing", and SQL has no valid `IN ()` form to render it to.
pub fn split_list_elements<'a>(source: &'a str, lhs: &str) -> Result<Vec<&'a str>, String> {
    let mut elements = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;

    for (index, character) in source.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == ',' => {
                elements.push(source[start..index].trim());
                start = index + character.len_utf8();
            }
            None => {}
        }
    }
    if quote.is_some() {
        return Err(format!(
            "unterminated string literal in the `in` list for `{lhs}`"
        ));
    }
    elements.push(source[start..].trim());

    if let Some(position) = elements.iter().position(|element| element.is_empty()) {
        return Err(if elements.len() == 1 {
            format!("`{lhs} in []` is empty; an `in` list needs at least one value")
        } else {
            format!(
                "empty value at position {} in the `in` list for `{lhs}` (a trailing or doubled comma)",
                position + 1
            )
        });
    }

    Ok(elements)
}

#[cfg(test)]
mod tests {
    use super::split_list_elements;

    #[test]
    fn commas_inside_a_string_literal_do_not_split() {
        assert_eq!(
            split_list_elements("\"a,b\", \"c\"", "label").as_deref(),
            Ok(["\"a,b\"", "\"c\""].as_slice())
        );
    }

    #[test]
    fn an_empty_list_is_rejected() {
        let error = split_list_elements("", "purpose").expect_err("empty list must be rejected");
        assert!(error.contains("at least one value"), "got: {error}");
    }

    #[test]
    fn a_trailing_comma_is_rejected() {
        let error =
            split_list_elements("a, ", "purpose").expect_err("trailing comma must be rejected");
        assert!(error.contains("position 2"), "got: {error}");
    }
}
