use super::{RESERVED, is_sql_reserved_word, is_sql_system_column};

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    let words: Vec<&str> = RESERVED.split_whitespace().collect();
    assert!(words.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn the_words_the_issue_names_are_reserved() {
    for word in ["user", "order", "group", "table", "select", "check"] {
        assert!(is_sql_reserved_word(word), "{word}");
    }
    for word in ["deposits", "map_bank", "ssn_enc", "account_id"] {
        assert!(!is_sql_reserved_word(word), "{word}");
    }
}

#[test]
fn the_postgres_and_sqlite_keyword_lists_are_covered() {
    // PostgreSQL "reserved" that the first list missed, and SQLite's `values`
    // and `last`.
    for word in [
        "true",
        "false",
        "values",
        "last",
        "current_role",
        "system_user",
    ] {
        assert!(is_sql_reserved_word(word), "{word}");
    }
}

#[test]
fn the_postgres_system_columns_are_named() {
    for name in ["xmin", "xmax", "cmin", "cmax", "ctid", "oid", "tableoid"] {
        assert!(is_sql_system_column(name), "{name}");
    }
    assert!(!is_sql_system_column("id"));
}
