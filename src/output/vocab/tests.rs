use super::symbol_kind;

#[test]
fn symbol_kinds_are_lower_cased_and_the_compound_ones_split() {
    let kinds = [
        "UNKNOWN",
        "FILE",
        "MODULE",
        "NAMESPACE",
        "PACKAGE",
        "CLASS",
        "METHOD",
        "PROPERTY",
        "FIELD",
        "CONSTRUCTOR",
        "ENUM",
        "INTERFACE",
        "FUNCTION",
        "VARIABLE",
        "CONSTANT",
        "STRING",
        "NUMBER",
        "BOOLEAN",
        "ARRAY",
        "OBJECT",
        "KEY",
        "NULL",
        "STRUCT",
        "EVENT",
        "OPERATOR",
    ];
    for kind in kinds {
        assert_eq!(symbol_kind(kind), kind.to_lowercase());
    }
    assert_eq!(symbol_kind("ENUMMEMBER"), "enum_member");
    assert_eq!(symbol_kind("TYPEPARAMETER"), "type_parameter");
    assert_eq!(symbol_kind("NEWKIND"), "newkind");
}
