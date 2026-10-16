use super::*;

fn header(name: &str, value: &str, enabled: bool) -> AiAgentHeader {
    AiAgentHeader {
        name: name.into(),
        value: value.into(),
        enabled,
    }
}

#[test]
fn accepts_flat_json_and_safe_js_literals_without_evaluation() {
    let rows = parse(r#"{Authorization: 'Bearer synthetic', 'X-Repeat': ["one", 'two',], "X-Escaped": '\u4e2d\ud83d\ude00\'"',}"#).unwrap();
    assert_eq!(
        rows,
        vec![
            header("Authorization", "Bearer synthetic", true),
            header("X-Repeat", "one", true),
            header("X-Repeat", "two", true),
            header("X-Escaped", "中😀'\"", true),
        ]
    );
    assert!(parse("{ }").unwrap().is_empty());
    assert_eq!(
        parse(r#"{"X-Repeat":"one","X-Repeat":"two"}"#)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn rejects_expressions_nested_values_and_malformed_sources_without_echoing_secrets() {
    for source in [
        "{Authorization: process.env.SECRET}",
        "{x: (() => 'synthetic-secret')()}",
        "{x: `synthetic-secret`}",
        "{x: {nested:'synthetic-secret'}}",
        "{x: 1}",
        "{x: true}",
        "{x: null}",
        "{x: []}",
        "{x: ['ok', 42]}",
        "{...secret}",
        "{get x() {return 'synthetic-secret'}}",
        "{x:'ok'};danger()",
        "{/* synthetic-secret */ x:'ok'}",
        "{x:'unfinished}",
        "{x:'bad\\x00'}",
        "{x:'bad\nnewline'}",
        "{x:'bad\\q'}",
        "{x: 'ok',,}",
        "[{}]",
        "",
    ] {
        let error = parse(source).unwrap_err();
        assert_eq!(format!("{error:?}"), "()");
    }
}

#[test]
fn limits_object_bytes_and_expanded_header_count_including_disabled_rows() {
    let rows = (0..32).map(|_| "'synthetic'").collect::<Vec<_>>().join(",");
    assert_eq!(parse(&format!("{{X:[{rows}]}}")).unwrap().len(), 32);
    assert!(parse(&format!("{{X:[{rows},'extra']}}")).is_err());
    let prefix = "{X:'";
    let suffix = "'}";
    let exact = format!(
        "{prefix}{}{suffix}",
        "a".repeat(MAX_OBJECT_BYTES - prefix.len() - suffix.len())
    );
    assert!(parse(&exact).is_ok());
    assert!(parse(&format!("{exact} ")).is_err());
    let disabled = vec![header("disabled", "retained", false)];
    assert!(merge(&format!("{{X:[{rows}]}}"), &disabled).is_err());
}

#[test]
fn unchanged_roundtrip_preserves_duplicates_disabled_rows_and_interleaved_order() {
    let original = vec![
        header("X-A", "one", true),
        header("bad name", "synthetic\r\n", false),
        header("X-B", "two", true),
        header("X-A", "three", true),
    ];
    let source = super::format(&original);
    assert!(source.contains(r#""X-A": ["one","three"]"#));
    assert!(!source.contains("synthetic"));
    assert_eq!(merge(&source, &original).unwrap(), original);
    let edited = merge("{ 'X-New': ['a', 'b'], }", &original).unwrap();
    assert_eq!(
        edited,
        vec![
            header("X-New", "a", true),
            original[1].clone(),
            header("X-New", "b", true)
        ]
    );
    assert_eq!(merge("{}", &original).unwrap(), vec![original[1].clone()]);
}
