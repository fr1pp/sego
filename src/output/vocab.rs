/// A `SymbolKind` value as sego names it: lower-cased, with the two
/// compound names split by an underscore.
pub fn symbol_kind(server: &str) -> String {
    match server {
        "ENUMMEMBER" => "enum_member".to_owned(),
        "TYPEPARAMETER" => "type_parameter".to_owned(),
        other => other.to_lowercase(),
    }
}

pub fn filter_kind(server: &str) -> String {
    match server {
        "commit date" => "commit_date".to_owned(),
        "symbol type" => "symbol_type".to_owned(),
        other => other.to_lowercase(),
    }
}
