use super::{RESERVED, is_sql_reserved_word};

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    assert!(RESERVED.windows(2).all(|pair| pair[0] < pair[1]));
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
