//! Declaration coverage for actual provider call sites; HTTP runtime limits are
//! separately exercised through the real adapters in the Release fixtures.

#[derive(Debug)]
struct Site {
    line: usize,
    operation: String,
    limit: Option<String>,
}

fn argument(source: &str, open: usize) -> (&str, usize) {
    assert_eq!(source.as_bytes()[open], b'(');
    let bytes = source.as_bytes();
    let mut depth = 1;
    let mut string = false;
    let mut escaped = false;
    let mut cursor = open + 1;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else if byte == b'"' {
            string = true;
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 {
                return (&source[open + 1..cursor], cursor + 1);
            }
        }
        cursor += 1;
    }
    panic!("unterminated RequestOptions builder argument");
}

fn sites(source: &str) -> Vec<Site> {
    let marker = "RequestOptions::new(";
    source
        .match_indices(marker)
        .map(|(start, _)| {
            let (operation, mut cursor) = argument(source, start + marker.len() - 1);
            let mut limit = None;
            loop {
                cursor += source[cursor..].len() - source[cursor..].trim_start().len();
                if source.as_bytes().get(cursor) != Some(&b'.') {
                    break;
                }
                cursor += 1;
                let name_start = cursor;
                while source
                    .as_bytes()
                    .get(cursor)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    cursor += 1;
                }
                let name = &source[name_start..cursor];
                if source.as_bytes().get(cursor) != Some(&b'(') {
                    break;
                }
                let (value, next) = argument(source, cursor);
                cursor = next;
                if name == "response_byte_limit" {
                    assert!(limit.is_none(), "multiple limits obscure call-site policy");
                    limit = Some(value.split_whitespace().collect());
                }
            }
            Site {
                line: source[..start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1,
                operation: operation.trim().trim_matches('"').to_string(),
                limit,
            }
        })
        .collect()
}

#[test]
fn builder_coverage_parser_preserves_chains_and_nested_arguments() {
    let source = r#"
        RequestOptions::new("operation (nested)")
            .retry_transient(3)
            .response_byte_limit(MAX_METADATA_RESPONSE_BYTES);
        RequestOptions::new(format!("{} (quoted)", provider))
            .response_byte_limit(MAX_DIFF_RESPONSE_BYTES).retry_transient(3);
    "#;
    let found = sites(source);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].operation, "operation (nested)");
    assert_eq!(
        found[0].limit.as_deref(),
        Some("MAX_METADATA_RESPONSE_BYTES")
    );
    assert_eq!(found[1].limit.as_deref(), Some("MAX_DIFF_RESPONSE_BYTES"));
}

#[test]
fn builder_coverage_parser_does_not_borrow_a_later_requests_limit() {
    let found = sites(
        r#"RequestOptions::new("unbounded").retry_transient(3),
        RequestOptions::new("bounded").response_byte_limit(1024);"#,
    );
    assert!(found[0].limit.is_none());
    assert_eq!(found[1].limit.as_deref(), Some("1024"));
}

#[test]
fn all_provider_requests_declare_their_class_and_keep_source_conflict_limits() {
    let sources = [
        ("github.rs", include_str!("github.rs"), 22),
        ("gitlab.rs", include_str!("gitlab.rs"), 23),
        ("gitee/mod.rs", include_str!("gitee/mod.rs"), 40),
        ("gitee/conflicts.rs", include_str!("gitee/conflicts.rs"), 3),
    ];
    let diff_operations = [
        "Load GitHub pull request files",
        "Compare GitHub pull request commits",
        "Load GitLab merge request diffs",
        "Compare GitLab merge request commits",
        "Load Gitee pull request diff",
        "Load Gitee pull request files",
        "Compare Gitee pull request commits",
        "Compare Gitee Enterprise pull request commits",
    ];
    let mut metadata = 0;
    let mut diff = 0;
    let mut protected = 0;
    for (file, source, count) in sources {
        let found = sites(source);
        assert_eq!(
            found.len(),
            count,
            "update the request inventory for {file}"
        );
        for site in found {
            let expected = match site.operation.as_str() {
                "Load GitHub conflict status"
                | "Load GitLab conflict status"
                | "Load Gitee conflict status"
                | "Resolve Gitee conflict revision" => {
                    protected += 1;
                    "1024*1024"
                }
                "Load Gitee conflict files" => {
                    protected += 1;
                    "16*1024*1024"
                }
                "Load GitHub repository file" | "Load Gitee repository file" => {
                    protected += 1;
                    "super::source::MAX_SOURCE_JSON_BYTES"
                }
                "Load GitLab repository file" => {
                    protected += 1;
                    "super::source::MAX_SOURCE_BYTES"
                }
                operation if diff_operations.contains(&operation) => {
                    diff += 1;
                    "MAX_DIFF_RESPONSE_BYTES"
                }
                _ => {
                    metadata += 1;
                    "MAX_METADATA_RESPONSE_BYTES"
                }
            };
            assert_eq!(
                site.limit.as_deref(),
                Some(expected),
                "{file}:{} operation={} has missing/wrong input policy",
                site.line,
                site.operation,
            );
        }
    }
    assert_eq!((metadata, diff, protected), (72, 8, 8));
}
