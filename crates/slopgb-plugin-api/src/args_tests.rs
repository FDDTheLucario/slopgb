use super::field;

#[test]
fn reads_string_fields() {
    let j = r#"{"from":"0100","to":"0103","view":"bg"}"#;
    assert_eq!(field(j, "from").as_deref(), Some("0100"));
    assert_eq!(field(j, "to").as_deref(), Some("0103"));
    assert_eq!(field(j, "view").as_deref(), Some("bg"));
    assert_eq!(field(j, "missing"), None);
}

#[test]
fn tolerates_whitespace_and_escapes() {
    let j = r#" { "expression" : "a+\t\"b\"" , "n" : 5 } "#;
    assert_eq!(field(j, "expression").as_deref(), Some("a+\t\"b\""));
    // A non-string value is skipped, not returned.
    assert_eq!(field(j, "n"), None);
    // A field after a skipped scalar is still found.
    assert_eq!(field(r#"{"n":5,"k":"v"}"#, "k").as_deref(), Some("v"));
}

#[test]
fn skips_nested_values_between_fields() {
    let j = r#"{"a":[1,2,{"x":"y"}],"b":"hit"}"#;
    assert_eq!(field(j, "b").as_deref(), Some("hit"));
}

#[test]
fn malformed_is_none_not_panic() {
    assert_eq!(field("not json", "k"), None);
    assert_eq!(field("{", "k"), None);
    assert_eq!(field(r#"{"k":"#, "k"), None);
    assert_eq!(field("[]", "k"), None);
}
