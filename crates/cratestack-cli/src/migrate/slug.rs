//! Slug sanitisation helpers for migration directory names.

/// Convert a developer-supplied slug to a filesystem-safe form:
/// lowercase, ASCII alphanumeric + underscore, no leading/trailing
/// underscores. Empty input falls back to "migration".
pub(super) fn sanitize_slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_underscore = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '_' || ch == '-' || ch == ' ' {
            Some('_')
        } else {
            None
        };
        if let Some(c) = mapped {
            if c == '_' {
                if prev_underscore || out.is_empty() {
                    continue;
                }
                prev_underscore = true;
            } else {
                prev_underscore = false;
            }
            out.push(c);
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "migration".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_sanitizer_normalizes_developer_input() {
        assert_eq!(sanitize_slug("Add Customer Email"), "add_customer_email");
        assert_eq!(sanitize_slug("--add--col--"), "add_col");
        assert_eq!(sanitize_slug(""), "migration");
        assert_eq!(sanitize_slug("@@!!"), "migration");
        assert_eq!(sanitize_slug("Order #42"), "order_42");
    }
}
