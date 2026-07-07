use super::*;

#[test]
fn renders_escaped_strings() {
    assert_eq!(Json::str("a\"b\\c\nd\te").render(), "\"a\\\"b\\\\c\\nd\\te\"");
    // A control char below 0x20 renders as a \u escape.
    assert_eq!(Json::str("\u{01}").render(), "\"\\u0001\"");
}

#[test]
fn renders_integral_numbers_without_decimal() {
    assert_eq!(Json::Num(1.0).render(), "1");
    assert_eq!(Json::Num(-42.0).render(), "-42");
    assert_eq!(Json::Num(19_483_529.0).render(), "19483529");
}

#[test]
fn renders_objects_and_arrays() {
    let v = Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("id", Json::Num(7.0)),
        ("arr", Json::Arr(vec![Json::Bool(true), Json::Null])),
    ]);
    assert_eq!(v.render(), r#"{"jsonrpc":"2.0","id":7,"arr":[true,null]}"#);
}

#[test]
fn parses_the_jsonrpc_envelope() {
    let v = parse(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"peek","arguments":{"from":"C000","to":"C00F"}}}"#,
    )
    .unwrap();
    assert_eq!(v.get("method").and_then(Json::as_str), Some("tools/call"));
    let args = v.get("params").and_then(|p| p.get("arguments")).unwrap();
    assert_eq!(args.get("from").and_then(Json::as_str), Some("C000"));
    assert_eq!(args.get("to").and_then(Json::as_str), Some("C00F"));
    assert_eq!(v.get("id"), Some(&Json::Num(1.0)));
}

#[test]
fn round_trips_through_parse_render() {
    let src = r#"{"a":"x\ny","b":[1,2,3],"c":{"d":true},"e":null}"#;
    assert_eq!(parse(src).unwrap().render(), src);
}

#[test]
fn malformed_input_errors_never_panics() {
    for bad in [
        "", "{", "[1,", r#"{"a":}"#, "truu", r#"{"a":1}x"#, "\"unterminated",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} should error");
    }
}

#[test]
fn deeply_nested_input_is_bounded_not_a_stack_overflow() {
    let deep = format!("{}{}", "[".repeat(500), "]".repeat(500));
    assert!(parse(&deep).is_err());
}

#[test]
fn parses_unicode_escape_and_raw_utf8() {
    assert_eq!(parse(r#""A""#).unwrap(), Json::str("A"));
    assert_eq!(parse("\"café\"").unwrap(), Json::str("café"));
}
