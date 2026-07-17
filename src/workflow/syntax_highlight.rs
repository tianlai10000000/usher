use std::sync::LazyLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::git::PullRequestDiffLine;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static GITHUB_THEME: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .remove("InspiredGitHub")
        .expect("syntect must bundle the InspiredGitHub theme")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HighlightedToken {
    pub(super) text: String,
    pub(super) color: [u8; 4],
    pub(super) dark_color: [u8; 4],
    pub(super) columns: u32,
    pub(super) indentation: bool,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HighlightedLine {
    pub(super) tokens: Vec<HighlightedToken>,
    pub(super) columns: u32,
}

pub(super) fn highlight_diff_lines(
    path: &str,
    lines: &[PullRequestDiffLine],
) -> Vec<HighlightedLine> {
    let extension = path_extension(path);
    let default_syntax = syntax_for_path(path);
    let mut syntax = default_syntax;
    let mut highlighter = HighlightLines::new(syntax, &GITHUB_THEME);
    let mut highlighted = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let expanded = expand_tabs(line.content.as_str(), 4);
        if matches!(line.kind.as_str(), "hunk" | "meta") {
            syntax = if extension == "vue" {
                syntax_for_vue_hunk(lines, index + 1)
            } else {
                default_syntax
            };
            highlighter = HighlightLines::new(syntax, &GITHUB_THEME);
            highlighted.push(plain_line(expanded, [0xff, 0x57, 0x60, 0x6a]));
            continue;
        }

        if extension == "vue" {
            if let Some(next_syntax) = vue_tag_next_syntax(expanded.trim()) {
                let mut tag_highlighter =
                    HighlightLines::new(syntax_by_name("HTML"), &GITHUB_THEME);
                let highlighted_input = format!("{expanded}\n");
                highlighted.push(
                    tag_highlighter
                        .highlight_line(highlighted_input.as_str(), &SYNTAX_SET)
                        .map(styled_line)
                        .unwrap_or_else(|_| plain_line(expanded, [0xff, 0x24, 0x29, 0x2f])),
                );
                syntax = next_syntax;
                highlighter = HighlightLines::new(syntax, &GITHUB_THEME);
                continue;
            }
        }

        let highlighted_input = format!("{expanded}\n");
        highlighted.push(
            match highlighter.highlight_line(highlighted_input.as_str(), &SYNTAX_SET) {
                Ok(ranges) => styled_line(ranges),
                Err(error) => {
                    log::debug!(
                        "[Workflow][PR Review] Syntax highlighting fallback: path={} error={}",
                        path,
                        error
                    );
                    plain_line(expanded, [0xff, 0x24, 0x29, 0x2f])
                }
            },
        );
    }
    highlighted
}

fn syntax_for_path(path: &str) -> &'static SyntaxReference {
    let extension = path_extension(path);
    if matches!(extension.as_str(), "ts" | "tsx" | "vue" | "svelte") {
        return syntax_by_name("JavaScript");
    }
    if matches!(extension.as_str(), "scss" | "sass") {
        return syntax_by_name("CSS");
    }
    if extension == "slint" {
        return syntax_by_name("Rust");
    }
    if let Ok(Some(syntax)) = SYNTAX_SET.find_syntax_for_file(path) {
        return syntax;
    }
    SYNTAX_SET.find_syntax_plain_text()
}

fn syntax_for_vue_hunk(lines: &[PullRequestDiffLine], start: usize) -> &'static SyntaxReference {
    let hunk = lines[start..]
        .iter()
        .take_while(|line| line.kind != "hunk")
        .map(|line| line.content.trim())
        .collect::<Vec<_>>();
    if hunk.first().is_some_and(|line| line.starts_with('<')) {
        return syntax_by_name("HTML");
    }
    let css_declaration_count = hunk
        .iter()
        .filter(|line| {
            line.contains(':')
                && !line.contains("=>")
                && !line.starts_with("const ")
                && !line.starts_with("let ")
                && !line.starts_with("function ")
        })
        .count();
    if css_declaration_count >= 2
        || hunk.iter().any(|line| {
            line.starts_with('.')
                || line.starts_with('#')
                || line.starts_with("--")
                || (line.contains(':') && line.ends_with(';'))
        })
    {
        return syntax_by_name("CSS");
    }
    syntax_by_name("JavaScript")
}

fn vue_tag_next_syntax(line: &str) -> Option<&'static SyntaxReference> {
    if line.starts_with("<script") {
        return Some(syntax_by_name("JavaScript"));
    }
    if line.starts_with("<style") {
        return Some(syntax_by_name("CSS"));
    }
    if line.starts_with("<template") || line.starts_with("</script") || line.starts_with("</style")
    {
        return Some(syntax_by_name("HTML"));
    }
    None
}

fn syntax_by_name(name: &str) -> &'static SyntaxReference {
    SYNTAX_SET
        .find_syntax_by_name(name)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text())
}

fn path_extension(path: &str) -> String {
    path.rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn styled_line(ranges: Vec<(Style, &str)>) -> HighlightedLine {
    let mut tokens = Vec::with_capacity(ranges.len() * 2);
    let mut columns = 0;
    for (style, value) in ranges {
        let value = value.strip_suffix('\n').unwrap_or(value);
        if value.is_empty() {
            continue;
        }
        let color = [
            style.foreground.a,
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ];
        let dark_color = github_dark_color(color);
        for (is_whitespace, segment) in split_whitespace_runs(value) {
            let token_columns = display_columns(segment);
            columns += token_columns;
            tokens.push(HighlightedToken {
                text: segment.to_string(),
                color,
                dark_color,
                columns: token_columns,
                indentation: is_whitespace,
                bold: !is_whitespace && style.font_style.contains(FontStyle::BOLD),
                italic: !is_whitespace && style.font_style.contains(FontStyle::ITALIC),
            });
        }
    }
    HighlightedLine { tokens, columns }
}

fn split_whitespace_runs(value: &str) -> Vec<(bool, &str)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut current_kind = None;
    for (index, character) in value.char_indices() {
        let is_whitespace = character.is_whitespace();
        if current_kind.is_some_and(|kind| kind != is_whitespace) {
            runs.push((current_kind.unwrap_or(false), &value[start..index]));
            start = index;
        }
        current_kind = Some(is_whitespace);
    }
    if let Some(kind) = current_kind {
        runs.push((kind, &value[start..]));
    }
    runs
}

fn plain_line(value: String, color: [u8; 4]) -> HighlightedLine {
    let columns = display_columns(value.as_str());
    let indentation = value.chars().all(char::is_whitespace);
    HighlightedLine {
        tokens: (!value.is_empty())
            .then(|| HighlightedToken {
                text: value,
                color,
                dark_color: github_dark_color(color),
                columns,
                indentation,
                bold: false,
                italic: false,
            })
            .into_iter()
            .collect(),
        columns,
    }
}

fn github_dark_color(color: [u8; 4]) -> [u8; 4] {
    let [alpha, red, green, blue] = color;
    let mapped = match [red, green, blue] {
        [0x24, 0x29, 0x2f] | [0x24, 0x29, 0x2e] => [0xe6, 0xed, 0xf3],
        [0x6a, 0x73, 0x7d] | [0x57, 0x60, 0x6a] => [0x8b, 0x94, 0x9e],
        [0x00, 0x5c, 0xc5] => [0x79, 0xc0, 0xff],
        [0x03, 0x2f, 0x62] => [0xa5, 0xd6, 0xff],
        [0x22, 0x86, 0x3a] => [0x7e, 0xe7, 0x87],
        [0x6f, 0x42, 0xc1] => [0xd2, 0xa8, 0xff],
        [0xd7, 0x3a, 0x49] | [0xb3, 0x1d, 0x28] => [0xff, 0x7b, 0x72],
        [0xe3, 0x62, 0x09] => [0xff, 0xa6, 0x57],
        _ => {
            let luminance =
                (u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114) / 1000;
            if luminance < 150 {
                [
                    red.saturating_add((255 - red) / 2),
                    green.saturating_add((255 - green) / 2),
                    blue.saturating_add((255 - blue) / 2),
                ]
            } else {
                [red, green, blue]
            }
        }
    };
    [alpha, mapped[0], mapped[1], mapped[2]]
}

fn expand_tabs(value: &str, tab_width: u32) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut column = 0;
    for character in value.chars() {
        if character == '\t' {
            let spaces = tab_width - column % tab_width;
            expanded.extend(std::iter::repeat_n(' ', spaces as usize));
            column += spaces;
        } else {
            expanded.push(character);
            column += character_columns(character);
        }
    }
    expanded
}

fn display_columns(value: &str) -> u32 {
    value.chars().map(character_columns).sum()
}

fn character_columns(character: char) -> u32 {
    if matches!(
        character as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
    ) {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        expand_tabs, github_dark_color, highlight_diff_lines, split_whitespace_runs,
        vue_tag_next_syntax,
    };
    use crate::git::PullRequestDiffLine;

    #[test]
    fn preserves_indent_and_highlights_rust_tokens() {
        let lines = vec![PullRequestDiffLine {
            old_line: 1,
            new_line: 1,
            kind: "context".to_string(),
            content: "\tlet answer: u32 = 42; // value".to_string(),
        }];

        let highlighted = highlight_diff_lines("src/lib.rs", &lines);
        assert_eq!(expand_tabs("\tx", 4), "    x");
        assert!(highlighted[0].tokens.len() > 2);
        assert_eq!(highlighted[0].tokens[0].text, "    ");
        assert_eq!(highlighted[0].tokens[0].columns, 4);
        assert!(highlighted[0].tokens[0].indentation);
        assert!(highlighted[0].columns >= 34);
    }

    #[test]
    fn highlights_typescript_with_javascript_fallback() {
        let lines = vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 1,
            kind: "addition".to_string(),
            content: "import value from 'package'; const count: number = 42;".to_string(),
        }];

        let highlighted = highlight_diff_lines("src/module.ts", &lines);
        let colors = highlighted[0]
            .tokens
            .iter()
            .map(|token| token.color)
            .collect::<HashSet<_>>();
        assert!(colors.len() >= 3);
    }

    #[test]
    fn separates_inter_token_whitespace_without_changing_source_text() {
        let source = "  min-height: 100vh;  // note";
        let runs = split_whitespace_runs(source);
        assert_eq!(runs.iter().map(|(_, run)| *run).collect::<String>(), source);
        assert_eq!(
            runs,
            vec![
                (true, "  "),
                (false, "min-height:"),
                (true, " "),
                (false, "100vh;"),
                (true, "  "),
                (false, "//"),
                (true, " "),
                (false, "note"),
            ]
        );
    }

    #[test]
    fn highlights_vue_template_hunks_as_html() {
        let lines = vec![
            PullRequestDiffLine {
                old_line: 0,
                new_line: 0,
                kind: "hunk".to_string(),
                content: "@@ -1,2 +1,2 @@ <template>".to_string(),
            },
            PullRequestDiffLine {
                old_line: 1,
                new_line: 1,
                kind: "context".to_string(),
                content: "  <div class=\"dialog\">{{ title }}</div>".to_string(),
            },
        ];

        let highlighted = highlight_diff_lines("src/Dialog.vue", &lines);
        assert!(highlighted[1].tokens.len() >= 4);
    }

    #[test]
    fn switches_vue_regions_and_uses_github_dark_contrast() {
        assert_eq!(
            vue_tag_next_syntax("<script setup lang=\"ts\">")
                .expect("script tag should select JavaScript")
                .name,
            "JavaScript"
        );
        assert_eq!(
            vue_tag_next_syntax("<style scoped>")
                .expect("style tag should select CSS")
                .name,
            "CSS"
        );
        assert_eq!(
            vue_tag_next_syntax("</style>")
                .expect("closing style tag should select HTML")
                .name,
            "HTML"
        );
        assert_eq!(
            github_dark_color([0xff, 0x24, 0x29, 0x2f]),
            [0xff, 0xe6, 0xed, 0xf3]
        );
        assert_eq!(
            github_dark_color([0xff, 0x6a, 0x73, 0x7d]),
            [0xff, 0x8b, 0x94, 0x9e]
        );
    }
}
