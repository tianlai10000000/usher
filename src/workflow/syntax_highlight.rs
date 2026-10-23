use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::git::{DiffLineKind, PullRequestDiffLine};

pub(super) struct HighlightResources {
    syntax_set: SyntaxSet,
    typescript_syntax_set: OnceLock<SyntaxSet>,
    github_theme: Theme,
}

impl HighlightResources {
    pub(super) fn load() -> Self {
        let started = std::time::Instant::now();
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let github_theme = ThemeSet::load_defaults()
            .themes
            .remove("InspiredGitHub")
            .expect("syntect must bundle the InspiredGitHub theme");
        log::info!(
            "[Workflow][PR Review] Loaded session syntax resources in {} ms",
            started.elapsed().as_millis()
        );
        Self {
            syntax_set,
            typescript_syntax_set: OnceLock::new(),
            github_theme,
        }
    }

    fn syntax_set_for_path(&self, path: &str) -> &SyntaxSet {
        if matches!(
            path_extension(path).as_str(),
            "ts" | "tsx" | "vue" | "svelte"
        ) {
            // Syntect's defaults omit TypeScript. JavaScript misparses typed
            // return values and can leave later source inside a template string.
            self.typescript_syntax_set
                .get_or_init(two_face::syntax::extra_newlines)
        } else {
            &self.syntax_set
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HighlightedToken {
    pub(super) text: String,
    pub(super) color: [u8; 4],
    pub(super) dark_color: [u8; 4],
    pub(super) bold: bool,
    pub(super) italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HighlightedLine {
    pub(super) tokens: Vec<HighlightedToken>,
    pub(super) columns: u32,
}

pub(super) fn highlight_diff_lines(
    resources: &HighlightResources,
    path: &str,
    lines: &[PullRequestDiffLine],
) -> Vec<HighlightedLine> {
    highlight_diff_lines_cancellable(resources, path, lines, || false)
        .expect("an uncancelled highlighter always completes")
}

/// A cancelled request never publishes a partial highlighted model.
/// Poll once per logical line; one Syntect line remains indivisible.
pub(super) fn highlight_diff_lines_cancellable(
    resources: &HighlightResources,
    path: &str,
    lines: &[PullRequestDiffLine],
    mut cancelled: impl FnMut() -> bool,
) -> Option<Vec<HighlightedLine>> {
    if cancelled() {
        return None;
    }
    let syntax_set = resources.syntax_set_for_path(path);
    let extension = path_extension(path);
    let default_syntax = syntax_for_path(syntax_set, path);
    let has_deletions = lines.iter().any(|line| line.kind == DiffLineKind::Deletion);
    // A diff interleaves two source files. Removed text must never change the
    // parser state used for additions or for the head version of context rows.
    let mut highlighters = [
        HighlightLines::new(default_syntax, &resources.github_theme),
        HighlightLines::new(default_syntax, &resources.github_theme),
    ];
    let mut highlighted = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if cancelled() {
            return None;
        }
        let expanded = expand_tabs(line.content.as_str(), 4);
        if line.kind == DiffLineKind::Hunk {
            for (side, highlighter) in highlighters.iter_mut().enumerate() {
                if side == 0 && !has_deletions {
                    continue;
                }
                let hunk = lines[index + 1..]
                    .iter()
                    .take_while(|line| line.kind != DiffLineKind::Hunk)
                    .filter(|line| source_side_includes(line.kind, side));
                let syntax = if extension == "vue" {
                    syntax_for_vue_hunk(syntax_set, hunk.clone())
                } else {
                    default_syntax
                };
                *highlighter = HighlightLines::new(syntax, &resources.github_theme);
                if matches!(
                    syntax.name.as_str(),
                    "JavaScript" | "TypeScript" | "TypeScriptReact" | "CSS"
                ) && hunk_starts_in_block_comment(hunk, &mut cancelled)?
                {
                    // The opener can be in hidden context. Prime only parser
                    // state; synthetic text must never enter paint or copy.
                    let _ = highlighter.highlight_line("/*\n", syntax_set);
                }
            }
            highlighted.push(plain_line(expanded, [0xff, 0x57, 0x60, 0x6a]));
            continue;
        }
        if line.kind == DiffLineKind::Meta {
            // A no-newline marker is not source and cannot end a comment/string.
            highlighted.push(plain_line(expanded, [0xff, 0x57, 0x60, 0x6a]));
            continue;
        }
        if has_deletions && line.kind == DiffLineKind::Context {
            highlight_source_line(
                resources,
                syntax_set,
                &mut highlighters[0],
                path,
                &extension,
                &expanded,
                false,
            );
        }
        let side = usize::from(line.kind != DiffLineKind::Deletion);
        highlighted.push(
            highlight_source_line(
                resources,
                syntax_set,
                &mut highlighters[side],
                path,
                &extension,
                &expanded,
                true,
            )
            .expect("a visible source line always produces tokens"),
        );
    }
    (!cancelled()).then_some(highlighted)
}

fn source_side_includes(kind: DiffLineKind, side: usize) -> bool {
    kind == DiffLineKind::Context
        || kind
            == if side == 0 {
                DiffLineKind::Deletion
            } else {
                DiffLineKind::Addition
            }
}

fn hunk_starts_in_block_comment<'a>(
    lines: impl Iterator<Item = &'a PullRequestDiffLine>,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<bool> {
    for line in lines {
        if cancelled() {
            return None;
        }
        let text = line.content.trim();
        if text.is_empty() {
            continue;
        }
        // Require a closing delimiter after an unbroken JSDoc-style prefix.
        // A multiplication continuation such as `* factor` alone is not enough.
        if text.starts_with("*/") {
            return Some(true);
        }
        if !text.starts_with('*') || text.starts_with("**") {
            return Some(false);
        }
        if text.contains("*/") {
            return Some(true);
        }
    }
    Some(false)
}

fn highlight_source_line<'a>(
    resources: &'a HighlightResources,
    syntax_set: &'a SyntaxSet,
    highlighter: &mut HighlightLines<'a>,
    path: &str,
    extension: &str,
    expanded: &str,
    render: bool,
) -> Option<HighlightedLine> {
    let input = format!("{expanded}\n");
    let result = if extension == "vue"
        && let Some(syntax) = vue_tag_next_syntax(syntax_set, expanded.trim())
    {
        *highlighter = HighlightLines::new(syntax, &resources.github_theme);
        HighlightLines::new(syntax_by_name(syntax_set, "HTML"), &resources.github_theme)
            .highlight_line(&input, syntax_set)
    } else {
        highlighter.highlight_line(&input, syntax_set)
    };
    render.then(|| {
        result.map(styled_line).unwrap_or_else(|error| {
            log::debug!(
                "[Workflow][PR Review] Syntax highlighting fallback: path={path} error={error}"
            );
            plain_line(expanded.to_owned(), [0xff, 0x24, 0x29, 0x2f])
        })
    })
}

fn syntax_for_path<'a>(syntax_set: &'a SyntaxSet, path: &str) -> &'a SyntaxReference {
    let extension = path_extension(path);
    if matches!(extension.as_str(), "ts" | "tsx") {
        return syntax_set
            .find_syntax_by_extension(&extension)
            .unwrap_or_else(|| syntax_by_name(syntax_set, "TypeScript"));
    }
    if matches!(extension.as_str(), "vue" | "svelte") {
        return syntax_by_name(syntax_set, "TypeScript");
    }
    if matches!(extension.as_str(), "scss" | "sass") {
        return syntax_by_name(syntax_set, "CSS");
    }
    if extension == "slint" {
        return syntax_by_name(syntax_set, "Rust");
    }
    if let Ok(Some(syntax)) = syntax_set.find_syntax_for_file(path) {
        return syntax;
    }
    syntax_set.find_syntax_plain_text()
}

fn syntax_for_vue_hunk<'a>(
    syntax_set: &'a SyntaxSet,
    lines: impl Iterator<Item = &'a PullRequestDiffLine>,
) -> &'a SyntaxReference {
    let hunk = lines.map(|line| line.content.trim()).collect::<Vec<_>>();
    if hunk.first().is_some_and(|line| line.starts_with('<')) {
        return syntax_by_name(syntax_set, "HTML");
    }
    if hunk.iter().any(|line| {
        [
            "import ",
            "export ",
            "const ",
            "let ",
            "function ",
            "interface ",
            "type ",
            "return ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    }) {
        return syntax_by_name(syntax_set, "TypeScript");
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
        return syntax_by_name(syntax_set, "CSS");
    }
    syntax_by_name(syntax_set, "TypeScript")
}

fn vue_tag_next_syntax<'a>(syntax_set: &'a SyntaxSet, line: &str) -> Option<&'a SyntaxReference> {
    if line.starts_with("<script") {
        let language = if line.contains("lang=\"ts\"") || line.contains("lang='ts'") {
            "TypeScript"
        } else {
            "JavaScript"
        };
        return Some(syntax_by_name(syntax_set, language));
    }
    if line.starts_with("<style") {
        return Some(syntax_by_name(syntax_set, "CSS"));
    }
    if line.starts_with("<template") || line.starts_with("</script") || line.starts_with("</style")
    {
        return Some(syntax_by_name(syntax_set, "HTML"));
    }
    None
}

fn syntax_by_name<'a>(syntax_set: &'a SyntaxSet, name: &str) -> &'a SyntaxReference {
    syntax_set
        .find_syntax_by_name(name)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
}

fn path_extension(path: &str) -> String {
    path.rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn styled_line(ranges: Vec<(Style, &str)>) -> HighlightedLine {
    let mut tokens = Vec::<HighlightedToken>::with_capacity(ranges.len());
    let mut columns = 0;
    let mut leading_whitespace = String::new();
    let mut whitespace_style = None;
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
        let bold = style.font_style.contains(FontStyle::BOLD);
        // GitHub's pull-request diff keeps source glyphs upright. Some syntect
        // grammars (notably Markdown) mark prose scopes as italic, which makes
        // complete added paragraphs look like emphasized text rather than code.
        // Normalize that theme detail here so paint and text geometry receive
        // the same upright token model.
        let italic = false;
        for (is_whitespace, segment) in split_whitespace_runs(value) {
            columns += display_columns(segment);
            if is_whitespace {
                if let Some(previous) = tokens.last_mut() {
                    previous.text.push_str(segment);
                } else {
                    leading_whitespace.push_str(segment);
                    whitespace_style = Some((color, dark_color, bold, italic));
                }
                continue;
            }

            let mut text = std::mem::take(&mut leading_whitespace);
            text.push_str(segment);
            append_compacted_token(&mut tokens, text, color, dark_color, bold, italic);
        }
    }
    if !leading_whitespace.is_empty() {
        let (color, dark_color, bold, italic) = whitespace_style.unwrap_or((
            [0xff, 0x24, 0x29, 0x2f],
            [0xff, 0xe6, 0xed, 0xf3],
            false,
            false,
        ));
        append_compacted_token(
            &mut tokens,
            leading_whitespace,
            color,
            dark_color,
            bold,
            italic,
        );
    }
    HighlightedLine { tokens, columns }
}

fn append_compacted_token(
    tokens: &mut Vec<HighlightedToken>,
    text: String,
    color: [u8; 4],
    dark_color: [u8; 4],
    bold: bool,
    italic: bool,
) {
    if let Some(previous) = tokens.last_mut()
        && previous.color == color
        && previous.dark_color == dark_color
        && previous.bold == bold
        && previous.italic == italic
    {
        previous.text.push_str(text.as_str());
        return;
    }
    tokens.push(HighlightedToken {
        text,
        color,
        dark_color,
        bold,
        italic,
    });
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
    HighlightedLine {
        tokens: (!value.is_empty())
            .then(|| HighlightedToken {
                text: value,
                color,
                dark_color: github_dark_color(color),
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

/// Expand tabs to spaces using the same stops as GitHub/syntax highlighting.
/// Diff selection, find, and clipboard must share this expansion with tokens.
pub(super) fn expand_tabs(value: &str, tab_width: u32) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut column = 0;
    for character in value.chars() {
        match character {
            '\r' | '\n' => {
                // Provider-normalized diff records represent exactly one
                // source line. Ignore defensive line-ending controls so a
                // malformed adapter response cannot create a multi-line Text
                // inside the fixed-height Slint row.
            }
            '\t' => {
                let spaces = tab_width - column % tab_width;
                expanded.extend(std::iter::repeat_n(' ', spaces as usize));
                column += spaces;
            }
            _ => {
                expanded.push(character);
                column += character_columns(character);
            }
        }
    }
    expanded
}

fn display_columns(value: &str) -> u32 {
    value.chars().map(character_columns).sum()
}

pub(super) fn character_columns(character: char) -> u32 {
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
mod comment_tests;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::LazyLock;

    use super::{
        HighlightResources, expand_tabs, github_dark_color, highlight_diff_lines,
        split_whitespace_runs, vue_tag_next_syntax,
    };
    use crate::git::PullRequestDiffLine;

    static TEST_RESOURCES: LazyLock<HighlightResources> = LazyLock::new(HighlightResources::load);

    #[test]
    fn preserves_indent_and_compacts_rust_tokens() {
        let lines = vec![PullRequestDiffLine {
            old_line: 1,
            new_line: 1,
            kind: crate::git::DiffLineKind::Context,
            content: "\tlet answer: u32 = 42; // value".to_string(),
        }];

        let highlighted = highlight_diff_lines(&TEST_RESOURCES, "src/lib.rs", &lines);
        assert_eq!(expand_tabs("\tx", 4), "    x");
        assert!(highlighted[0].tokens.len() > 2);
        assert!(highlighted[0].tokens[0].text.starts_with("    let"));
        assert!(
            highlighted[0]
                .tokens
                .iter()
                .all(|token| !token.text.chars().all(char::is_whitespace))
        );
        assert_eq!(
            highlighted[0]
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "    let answer: u32 = 42; // value"
        );
        assert!(highlighted[0].columns >= 34);
    }

    #[test]
    fn highlights_typescript_with_its_own_grammar() {
        let lines = vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 1,
            kind: crate::git::DiffLineKind::Addition,
            content: "import value from 'package'; const count: number = 42;".to_string(),
        }];

        let highlighted = highlight_diff_lines(&TEST_RESOURCES, "src/module.ts", &lines);
        let colors = highlighted[0]
            .tokens
            .iter()
            .map(|token| token.color)
            .collect::<HashSet<_>>();
        assert!(colors.len() >= 3);
    }

    #[test]
    fn keeps_markdown_diff_source_upright() {
        let lines = vec![PullRequestDiffLine {
            old_line: 0,
            new_line: 1,
            kind: crate::git::DiffLineKind::Addition,
            content: "The current product supports the following workflow:".to_string(),
        }];

        let highlighted = highlight_diff_lines(&TEST_RESOURCES, "docs/specs/pr-review.md", &lines);
        assert!(
            highlighted[0].tokens.iter().all(|token| !token.italic),
            "PR diff syntax highlighting must not italicize Markdown prose"
        );
    }

    #[test]
    fn strips_provider_line_endings_before_highlighting() {
        let lines = vec![PullRequestDiffLine {
            old_line: 1,
            new_line: 1,
            kind: crate::git::DiffLineKind::Context,
            content: "const value =\r\n'中文';\n".to_string(),
        }];

        let highlighted = highlight_diff_lines(&TEST_RESOURCES, "src/module.ts", &lines);
        let rendered = highlighted[0]
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        assert_eq!(rendered, "const value ='中文';");
        assert!(!rendered.contains(['\r', '\n']));
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
                kind: crate::git::DiffLineKind::Hunk,
                content: "@@ -1,2 +1,2 @@ <template>".to_string(),
            },
            PullRequestDiffLine {
                old_line: 1,
                new_line: 1,
                kind: crate::git::DiffLineKind::Context,
                content: "  <div class=\"dialog\">{{ title }}</div>".to_string(),
            },
        ];

        let highlighted = highlight_diff_lines(&TEST_RESOURCES, "src/Dialog.vue", &lines);
        assert!(highlighted[1].tokens.len() >= 4);
    }

    #[test]
    fn switches_vue_regions_and_uses_github_dark_contrast() {
        let syntax_set = TEST_RESOURCES.syntax_set_for_path("src/Dialog.vue");
        assert_eq!(
            vue_tag_next_syntax(syntax_set, "<script setup lang=\"ts\">")
                .expect("typed script tag should select TypeScript")
                .name,
            "TypeScript"
        );
        assert_eq!(
            vue_tag_next_syntax(syntax_set, "<style scoped>")
                .expect("style tag should select CSS")
                .name,
            "CSS"
        );
        assert_eq!(
            vue_tag_next_syntax(syntax_set, "</style>")
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
