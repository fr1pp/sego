//! The upstream's vocabulary in sego's terms: symbol kinds, filter kinds
//! and the alert titles sego recognises.

use crate::domain::Term;

/// The alert for a query no repository matches.
pub const ALERT_NO_REPOS: &str = "No repositories found";
/// The alert for a query the upstream can't run.
pub const ALERT_BAD_QUERY: &str = "Unable To Process Query";
/// The alert for a query at a revision that doesn't exist.
pub const ALERT_MISSING_REVISION: &str =
    "Some revisions could not be searched";

/// `SymbolKind`'s values, as `just schema` lists them, and sego's terms.
const SYMBOL_KINDS: &[(&str, &str)] = &[
    ("UNKNOWN", "unknown"),
    ("FILE", "file"),
    ("MODULE", "module"),
    ("NAMESPACE", "namespace"),
    ("PACKAGE", "package"),
    ("CLASS", "class"),
    ("METHOD", "method"),
    ("PROPERTY", "property"),
    ("FIELD", "field"),
    ("CONSTRUCTOR", "constructor"),
    ("ENUM", "enum"),
    ("INTERFACE", "interface"),
    ("FUNCTION", "function"),
    ("VARIABLE", "variable"),
    ("CONSTANT", "constant"),
    ("STRING", "string"),
    ("NUMBER", "number"),
    ("BOOLEAN", "boolean"),
    ("ARRAY", "array"),
    ("OBJECT", "object"),
    ("KEY", "key"),
    ("NULL", "null"),
    ("ENUMMEMBER", "enum_member"),
    ("STRUCT", "struct"),
    ("EVENT", "event"),
    ("OPERATOR", "operator"),
    ("TYPEPARAMETER", "type_parameter"),
];

/// The kinds of filter the upstream suggests, and sego's terms. The
/// upstream's are free text, not a schema enum.
const FILTER_KINDS: &[(&str, &str)] = &[
    ("repo", "repo"),
    ("file", "file"),
    ("lang", "lang"),
    ("type", "type"),
    ("utility", "utility"),
    ("author", "author"),
    ("commit date", "commit_date"),
    ("symbol type", "symbol_type"),
];

/// A symbol's `SymbolKind` in sego's terms, or why it has none: a value
/// outside the schema's enum is drift.
pub fn symbol_kind(raw: &str) -> Result<&'static str, String> {
    SYMBOL_KINDS
        .iter()
        .find(|(upstream, _)| *upstream == raw)
        .map(|&(_, value)| value)
        .ok_or_else(|| format!("unknown symbol kind `{raw}`"))
}

/// A suggested filter's kind.
pub fn filter_kind(raw: &str) -> Term {
    term(FILTER_KINDS, raw)
}

/// `raw` in sego's terms, by `table`; `other` if the table lacks it.
fn term(table: &[(&str, &'static str)], raw: &str) -> Term {
    match table.iter().find(|(upstream, _)| *upstream == raw) {
        Some(&(_, value)) => Term { value, raw: None },
        None => Term { value: "other", raw: Some(raw.to_owned()) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(value: &'static str) -> Term {
        Term { value, raw: None }
    }

    fn other(raw: &str) -> Term {
        Term { value: "other", raw: Some(raw.to_owned()) }
    }

    #[test]
    fn symbol_kinds() {
        assert_eq!(symbol_kind("METHOD"), Ok("method"));
        assert_eq!(symbol_kind("ENUMMEMBER"), Ok("enum_member"));
        assert_eq!(symbol_kind("TYPEPARAMETER"), Ok("type_parameter"));
        assert_eq!(symbol_kind("UNKNOWN"), Ok("unknown"));
        assert_eq!(
            symbol_kind("NEWKIND"),
            Err("unknown symbol kind `NEWKIND`".to_owned())
        );
        assert!(symbol_kind("method").is_err());
    }

    #[test]
    fn filter_kinds() {
        assert_eq!(filter_kind("repo"), term("repo"));
        assert_eq!(filter_kind("commit date"), term("commit_date"));
        assert_eq!(filter_kind("symbol type"), term("symbol_type"));
        assert_eq!(filter_kind("newkind"), other("newkind"));
    }
}
