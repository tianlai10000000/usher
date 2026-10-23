use std::sync::LazyLock;

use super::*;
use crate::git::DiffLineKind;

static RESOURCES: LazyLock<HighlightResources> = LazyLock::new(HighlightResources::load);

fn source_lines(source: &[&str]) -> Vec<PullRequestDiffLine> {
    diff_lines(
        &source
            .iter()
            .map(|line| (DiffLineKind::Context, *line))
            .collect::<Vec<_>>(),
    )
}

fn diff_lines(source: &[(DiffLineKind, &str)]) -> Vec<PullRequestDiffLine> {
    let mut old_line = 0;
    let mut new_line = 0;
    source
        .iter()
        .map(|(kind, content)| {
            if matches!(kind, DiffLineKind::Context | DiffLineKind::Deletion) {
                old_line += 1;
            }
            if matches!(kind, DiffLineKind::Context | DiffLineKind::Addition) {
                new_line += 1;
            }
            PullRequestDiffLine {
                old_line: if *kind == DiffLineKind::Addition {
                    0
                } else {
                    old_line
                },
                new_line: if *kind == DiffLineKind::Deletion {
                    0
                } else {
                    new_line
                },
                kind: *kind,
                content: (*content).to_owned(),
            }
        })
        .collect()
}

fn assert_source_preserved(lines: &[PullRequestDiffLine], highlighted: &[HighlightedLine]) {
    assert_eq!(lines.len(), highlighted.len());
    for (line, highlighted) in lines.iter().zip(highlighted) {
        assert_eq!(
            highlighted
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            expand_tabs(&line.content, 4),
        );
    }
}

#[test]
fn hunk_starting_inside_doc_comment_matches_complete_source() {
    let visible = [
        " * 上一段文档注释的结尾",
        " */",
        "/** 通知WebView打开个人活动量月报 */",
        "export function openPersonalMonthlyReport(data: {",
        "  salesCode: string",
        "  salesName: string",
        "}): void {",
        "  const message = 'openPersonalMonthlyReport';",
        "  window.ReactNativeWebView?.postMessage(message);",
        "}",
        "const count = 42;",
    ];
    for path in [
        "src/report.js",
        "src/report.ts",
        "src/report.tsx",
        "src/Report.vue",
    ] {
        let mut complete = vec!["/**"];
        complete.extend(visible);
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&complete));
        let mut input = vec![(DiffLineKind::Hunk, "@@ -20,11 +20,11 @@")];
        input.extend(visible.iter().map(|line| (DiffLineKind::Context, *line)));
        let lines = diff_lines(&input);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        assert_source_preserved(&lines, &actual);
        assert_eq!(&actual[1..], &expected[1..], "{path}");
    }
}

#[test]
fn hunk_starting_with_comment_terminator_does_not_open_a_regexp() {
    let visible = [
        " */",
        "/** 通知WebView打开业务查询 */",
        "export const report = 'monthly';",
        "const count = 42;",
    ];
    for path in [
        "src/report.js",
        "src/report.ts",
        "src/report.tsx",
        "src/Report.vue",
    ] {
        let mut complete = vec!["/* hidden comment"];
        complete.extend(visible);
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&complete));
        let mut input = vec![(DiffLineKind::Hunk, "@@ -30,4 +30,4 @@")];
        input.extend(visible.iter().map(|line| (DiffLineKind::Context, *line)));
        let lines = diff_lines(&input);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        assert_source_preserved(&lines, &actual);
        assert_eq!(&actual[1..], &expected[1..], "{path}");
    }
}

#[test]
fn typed_async_return_followed_by_template_does_not_leak_string_scope() {
    // The JavaScript fallback misreads the generic return type and the call's
    // parentheses. The final template backtick then opens a string instead of
    // closing it, making the following comment and all later code string blue.
    let source = [
        "async function previous(): Promise<boolean> {",
        "  return Boolean()",
        "}",
        "function next(): string {",
        "  return `${1}:${2}`",
        "}",
        "/** 通知WebView打开个人活动量月报 */",
        "export const after = 42;",
    ];
    for path in ["src/report.ts", "src/report.tsx", "src/Report.vue"] {
        let vue = path.ends_with(".vue");
        let mut input = vec![(DiffLineKind::Hunk, "@@ -0,0 +1,8 @@")];
        let mut reference = Vec::new();
        if vue {
            input.push((DiffLineKind::Addition, "<script setup lang=\"ts\">"));
            reference.push("<script setup lang=\"ts\">");
        }
        let source_offset = input.len();
        input.extend(source.iter().map(|line| (DiffLineKind::Addition, *line)));
        reference.extend(&source[6..]);
        if vue {
            input.push((DiffLineKind::Addition, "</script>"));
            reference.push("</script>");
        }
        let lines = diff_lines(&input);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&reference));
        let reference_offset = usize::from(vue);
        assert_source_preserved(&lines, &actual);
        assert_eq!(
            &actual[source_offset + 6..source_offset + 8],
            &expected[reference_offset..reference_offset + 2],
            "{path}"
        );
        assert!(
            actual[source_offset + 7]
                .tokens
                .iter()
                .map(|token| token.color)
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 3,
            "{path}: code after the comment must retain distinct token colors"
        );
    }
}

#[test]
fn complete_multiline_comments_keep_their_scope_until_the_terminator() {
    let source = [
        "/**",
        " * 中文说明、`template` 和 /regexp/ 都属于注释",
        " * const fake = 'comment';",
        " */",
        "export const value = 'after';",
    ];
    for path in ["src/report.js", "src/report.ts", "src/Report.vue"] {
        let lines = source_lines(&source);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&source[4..]));
        assert_source_preserved(&lines, &actual);
        assert_eq!(&actual[4..], expected, "{path}");
        assert!(
            actual[1]
                .tokens
                .iter()
                .all(|token| token.color == actual[0].tokens[0].color)
        );
    }
}

fn assert_diff_sides_match_sources(input: &[(DiffLineKind, &str)]) {
    let lines = diff_lines(input);
    let old = lines
        .iter()
        .filter(|line| !matches!(line.kind, DiffLineKind::Hunk | DiffLineKind::Addition))
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();
    let new = lines
        .iter()
        .filter(|line| !matches!(line.kind, DiffLineKind::Hunk | DiffLineKind::Deletion))
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();
    for path in ["src/report.js", "src/report.ts", "src/Report.vue"] {
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        let expected_old = highlight_diff_lines(&RESOURCES, path, &source_lines(&old));
        let expected_new = highlight_diff_lines(&RESOURCES, path, &source_lines(&new));
        assert_source_preserved(&lines, &actual);
        for (index, line) in lines.iter().enumerate() {
            let expected = match line.kind {
                DiffLineKind::Hunk => continue,
                DiffLineKind::Deletion => &expected_old[line.old_line as usize - 1],
                _ => &expected_new[line.new_line as usize - 1],
            };
            assert_eq!(&actual[index], expected, "{path}: {}", line.content);
        }
    }
}

#[test]
fn changed_multiline_template_openers_keep_old_and_new_parsers_separate() {
    assert_diff_sides_match_sources(&[
        (DiffLineKind::Hunk, "@@ -1,6 +1,6 @@"),
        (DiffLineKind::Context, "const message ="),
        (DiffLineKind::Deletion, "`old"),
        (DiffLineKind::Addition, "`new"),
        (DiffLineKind::Context, "value`;"),
        (
            DiffLineKind::Context,
            "/** 通知WebView打开个人活动量月报 */",
        ),
        (DiffLineKind::Context, "export const count = 42;"),
    ]);
}

#[test]
fn replacing_a_template_with_a_block_comment_keeps_each_side_valid() {
    assert_diff_sides_match_sources(&[
        (DiffLineKind::Hunk, "@@ -1,7 +1,7 @@"),
        (DiffLineKind::Context, "const message ="),
        (DiffLineKind::Deletion, "`old"),
        (DiffLineKind::Addition, "/* new"),
        (DiffLineKind::Context, " * shared"),
        (DiffLineKind::Deletion, "value`;"),
        (DiffLineKind::Addition, " */ 'new';"),
        (DiffLineKind::Context, "/** 通知WebView打开业务查询 */"),
        (DiffLineKind::Context, "export const count = 42;"),
    ]);
}

#[test]
fn multiplication_continuations_are_not_inferred_to_be_doc_comments() {
    let source = [
        "  * multiplier",
        "  + offset;",
        "/** 中文注释 */",
        "export const count = 42;",
    ];
    for path in ["src/report.js", "src/report.ts", "src/Report.vue"] {
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&source));
        let mut input = vec![(DiffLineKind::Hunk, "@@ -30,4 +30,4 @@")];
        input.extend(source.iter().map(|line| (DiffLineKind::Context, *line)));
        let lines = diff_lines(&input);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        assert_source_preserved(&lines, &actual);
        assert_eq!(&actual[1..], expected, "{path}");
    }
}

#[test]
fn comment_looking_markers_in_strings_and_regexps_do_not_reset_the_parser() {
    let source = [
        "const message = `before",
        " */",
        "/** 这里仍是字符串 */",
        "after`;",
        "const markers = /\\/\\*\\*.*\\*\\//;",
        "const quoted = '/** 这里也是字符串 */';",
        "/** 这里才是注释 */",
        "export const count = 42;",
    ];
    for path in ["src/report.js", "src/report.ts", "src/Report.vue"] {
        let expected = highlight_diff_lines(&RESOURCES, path, &source_lines(&source));
        let mut input = vec![(DiffLineKind::Hunk, "@@ -1,8 +1,8 @@")];
        input.extend(source.iter().map(|line| (DiffLineKind::Context, *line)));
        let lines = diff_lines(&input);
        let actual = highlight_diff_lines(&RESOURCES, path, &lines);
        assert_source_preserved(&lines, &actual);
        assert_eq!(&actual[1..], expected, "{path}");
        assert_eq!(actual[2].tokens[0].color, actual[3].tokens[0].color);
    }
}

#[test]
fn vue_css_hunks_continue_to_use_css_highlighting() {
    let source = [
        ".report {",
        "  content: \"=>\";",
        "  min-height: 100vh;",
        "}",
    ];
    let mut complete = vec!["<style scoped>"];
    complete.extend(source);
    let expected = highlight_diff_lines(&RESOURCES, "src/Report.vue", &source_lines(&complete));
    let mut input = vec![(DiffLineKind::Hunk, "@@ -40,4 +40,4 @@")];
    input.extend(source.iter().map(|line| (DiffLineKind::Context, *line)));
    let lines = diff_lines(&input);
    let actual = highlight_diff_lines(&RESOURCES, "src/Report.vue", &lines);
    assert_source_preserved(&lines, &actual);
    assert_eq!(&actual[1..], &expected[1..]);
}
