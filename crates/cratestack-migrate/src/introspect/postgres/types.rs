//! Postgres type name → `.cstack` scalar name mapping.
//!
//! Deliberately a small whitelist, not a general SQL-type parser. Per
//! the design doc's explicit "unmapped → reported drift, never
//! guessed" rule (§5.2, §6 open question 3): only the exact Postgres
//! type each `.cstack` scalar's own emitter produces
//! (`crate::emit::postgres::columns::scalar_to_postgres`) round-trips
//! back to that scalar. Anything else — narrower/wider integer widths,
//! `varchar`/`bpchar` instead of `text`, plain `timestamp` instead of
//! `timestamptz`, `numeric`, `jsonb`, arrays, domains, extension types
//! — is reported as unmapped by the caller rather than guessed at,
//! because guessing risks under-reporting real drift (design doc §9):
//! an `int4` column silently mapped to `Int` would make a live table
//! that actually differs from the schema (`Int` always emits `int8`)
//! look identical to it.
pub(super) fn map_scalar(typname: &str, typtype: char, typcategory: char) -> Option<&'static str> {
    // `typtype != 'b'` excludes enums (`'e'`, handled separately via
    // `pg_enum` — see `super::enums`), domains (`'d'`), composite
    // types (`'c'`), and pseudo-types (`'p'`). `typcategory == 'A'`
    // excludes every array type regardless of element type — Postgres
    // represents `text[]` as its own base type (`_text`) with
    // `typtype = 'b'`, so the array check has to be separate from the
    // `typtype` one.
    if typtype != 'b' || typcategory == 'A' {
        return None;
    }
    match typname {
        "text" => Some("String"),
        "uuid" => Some("Uuid"),
        "timestamptz" => Some("DateTime"),
        "bool" => Some("Boolean"),
        "int8" => Some("Int"),
        "float8" => Some("Float"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_common_scalar_set() {
        assert_eq!(map_scalar("text", 'b', 'S'), Some("String"));
        assert_eq!(map_scalar("uuid", 'b', 'U'), Some("Uuid"));
        assert_eq!(map_scalar("timestamptz", 'b', 'D'), Some("DateTime"));
        assert_eq!(map_scalar("bool", 'b', 'B'), Some("Boolean"));
        assert_eq!(map_scalar("int8", 'b', 'N'), Some("Int"));
        assert_eq!(map_scalar("float8", 'b', 'N'), Some("Float"));
    }

    #[test]
    fn narrower_int_widths_are_unmapped_not_guessed() {
        // `Int` always emits `int8` (BIGINT) — see `scalar_to_postgres`
        // — so a live `int4`/`int2` column is genuinely different, not
        // an equivalent spelling of `Int`.
        assert_eq!(map_scalar("int4", 'b', 'N'), None);
        assert_eq!(map_scalar("int2", 'b', 'N'), None);
    }

    #[test]
    fn numeric_jsonb_bytea_are_unmapped() {
        assert_eq!(map_scalar("numeric", 'b', 'N'), None);
        assert_eq!(map_scalar("jsonb", 'b', 'U'), None);
        assert_eq!(map_scalar("bytea", 'b', 'U'), None);
    }

    #[test]
    fn arrays_are_unmapped_regardless_of_element_type() {
        assert_eq!(map_scalar("_text", 'b', 'A'), None);
        assert_eq!(map_scalar("_int8", 'b', 'A'), None);
    }

    #[test]
    fn domains_and_enums_are_unmapped_by_this_function() {
        // Enums get their own reconstruction path (`super::enums`);
        // this function only ever sees `typtype = 'b'` for a column
        // that's actually going to be treated as a mapped scalar.
        assert_eq!(map_scalar("text", 'd', 'S'), None);
        assert_eq!(map_scalar("order_status", 'e', 'E'), None);
    }
}
