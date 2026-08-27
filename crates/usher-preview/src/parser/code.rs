use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::{CancellationToken, MarkdownCodeSpan, PreviewError, PreviewPreparationProgress};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ParserLoadStrategy {
    #[default]
    OnDemand,
    PreloadAll,
}

pub trait CodeParser: Send + Sync {
    fn highlight(
        &self,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MarkdownCodeSpan>, PreviewError>;
}

pub trait CodeParserLoader: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports(&self, language: &str) -> bool;
    fn cache_key(&self, language: &str) -> String {
        language.to_string()
    }
    fn display_name(&self, language: &str) -> String;
    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError>;
    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError>;
}

pub struct CodeParserRegistry {
    strategy: ParserLoadStrategy,
    loaders: Vec<Arc<dyn CodeParserLoader>>,
    loaded: Mutex<HashMap<(&'static str, String), Arc<dyn CodeParser>>>,
    preload_complete: Mutex<bool>,
}

impl CodeParserRegistry {
    pub fn new(strategy: ParserLoadStrategy) -> Self {
        Self {
            strategy,
            loaders: Vec::new(),
            loaded: Mutex::new(HashMap::new()),
            preload_complete: Mutex::new(false),
        }
    }

    pub fn with_builtins(strategy: ParserLoadStrategy) -> Self {
        let mut registry = Self::new(strategy);
        registry.register(TsParserLoader::default());
        registry.register(JavaScriptParserLoader::default());
        registry.register(static_tree_sitter_loader(
            "rust",
            &["rs", "rust"],
            "Rust",
            "fn main() { let ready = true; }\n",
            build_rust_parser,
        ));
        registry.register(static_tree_sitter_loader(
            "shell",
            &["sh", "bash", "shell", "shellscript", "zsh"],
            "Shell",
            "set -euo pipefail\nprintf '%s\\n' ready\n",
            build_shell_parser,
        ));
        registry.register(static_tree_sitter_loader(
            "json",
            &["json"],
            "JSON",
            "{\"ready\": true, \"count\": 1}\n",
            build_json_parser,
        ));
        registry.register(static_tree_sitter_loader(
            "python",
            &["py", "python", "python3"],
            "Python",
            "def ready(value: int) -> bool:\n    return value > 0\n",
            build_python_parser,
        ));
        registry.register(static_tree_sitter_loader(
            "go",
            &["go", "golang"],
            "Go",
            "func ready(value int) bool { return value > 0 }\n",
            build_go_parser,
        ));
        registry.register(PlainTextParserLoader::default());
        registry
    }

    pub fn register(&mut self, loader: impl CodeParserLoader + 'static) {
        let id = loader.id();
        self.loaders.retain(|existing| existing.id() != id);
        self.loaders.push(Arc::new(loader));
    }

    pub fn strategy(&self) -> ParserLoadStrategy {
        self.strategy
    }

    pub fn prepare(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        if self.strategy != ParserLoadStrategy::PreloadAll {
            return cancellation.check();
        }

        let mut complete = self
            .preload_complete
            .lock()
            .expect("code parser preload lock poisoned");
        if *complete {
            return cancellation.check();
        }
        for loader in &self.loaders {
            cancellation.check()?;
            loader.preload(cancellation)?;
        }
        cancellation.check()?;
        *complete = true;
        Ok(())
    }

    pub fn highlight(
        &self,
        source: &str,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
        cancellation.check()?;
        let language = normalize_language(language);
        let loader = self
            .loaders
            .iter()
            .find(|loader| loader.supports(&language))
            .ok_or_else(|| {
                PreviewError::Decode(format!(
                    "no code parser loader accepts language '{language}'"
                ))
            })?;
        let cache_key = (loader.id(), loader.cache_key(&language));
        let parser = {
            let mut loaded = self.loaded.lock().expect("code parser cache lock poisoned");
            if let Some(parser) = loaded.get(&cache_key) {
                parser.clone()
            } else {
                let loader_id = loader.id();
                let display_name = loader.display_name(&language);
                cancellation.report_progress(PreviewPreparationProgress::ParserLoading {
                    language: language.clone(),
                    loader_id,
                    display_name: display_name.clone(),
                });
                let parser = loader.load(&language, cancellation)?;
                cancellation.check()?;
                loaded.insert(cache_key, parser.clone());
                cancellation.report_progress(PreviewPreparationProgress::ParserReady {
                    language: language.clone(),
                    loader_id,
                    display_name,
                });
                parser
            }
        };
        parser.highlight(source, cancellation)
    }

    pub fn loaded_parser_count(&self) -> usize {
        self.loaded
            .lock()
            .expect("code parser cache lock poisoned")
            .len()
    }

    #[cfg(test)]
    fn loader_id_for(&self, language: &str) -> Option<&'static str> {
        let language = normalize_language(language);
        self.loaders
            .iter()
            .find(|loader| loader.supports(&language))
            .map(|loader| loader.id())
    }
}

fn normalize_language(language: &str) -> String {
    language.trim().to_ascii_lowercase()
}

fn language_display_name(language: &str) -> String {
    match language {
        "" => "Plain text".to_string(),
        "js" | "mjs" | "cjs" | "javascript" => "JavaScript".to_string(),
        "jsx" | "javascriptreact" => "JSX".to_string(),
        "ts" | "typescript" => "TypeScript".to_string(),
        "tsx" | "typescriptreact" => "TSX".to_string(),
        "rs" | "rust" => "Rust".to_string(),
        "sh" | "bash" | "shell" | "shellscript" | "zsh" => "Shell".to_string(),
        "json" => "JSON".to_string(),
        "py" | "python" | "python3" => "Python".to_string(),
        "go" | "golang" => "Go".to_string(),
        "html" => "HTML".to_string(),
        "css" => "CSS".to_string(),
        other => {
            let mut characters = other.chars();
            let Some(first) = characters.next() else {
                return "Plain text".to_string();
            };
            first.to_uppercase().chain(characters).collect()
        }
    }
}

#[derive(Default)]
pub struct TsParserLoader {
    typescript: OnceLock<Arc<TreeSitterCodeParser>>,
    tsx: OnceLock<Arc<TreeSitterCodeParser>>,
}

impl CodeParserLoader for TsParserLoader {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn supports(&self, language: &str) -> bool {
        matches!(language, "ts" | "typescript" | "tsx" | "typescriptreact")
    }

    fn cache_key(&self, language: &str) -> String {
        if matches!(language, "tsx" | "typescriptreact") {
            "tsx"
        } else {
            "typescript"
        }
        .to_string()
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        let tsx = matches!(language, "tsx" | "typescriptreact");
        let slot = if tsx { &self.tsx } else { &self.typescript };
        let parser = load_tree_sitter_parser(slot, cancellation, || build_typescript_parser(tsx))?;
        Ok(parser)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let parser = self.load("typescript", cancellation)?;
        let _ = parser.highlight("const value: number = 1;\n", cancellation)?;
        let parser = self.load("tsx", cancellation)?;
        let _ = parser.highlight("const view = <main>{value}</main>;\n", cancellation)?;
        Ok(())
    }
}

const TREE_SITTER_HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "constructor",
    "constructor.builtin",
    "embedded",
    "error",
    "escape",
    "function",
    "function.builtin",
    "keyword",
    "markup",
    "markup.bold",
    "markup.heading",
    "markup.italic",
    "markup.link",
    "markup.link.url",
    "markup.list",
    "markup.quote",
    "markup.raw",
    "markup.strikethrough",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regexp",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

struct TreeSitterCodeParser {
    configuration: HighlightConfiguration,
}

impl TreeSitterCodeParser {
    fn new(mut configuration: HighlightConfiguration) -> Self {
        configuration.configure(TREE_SITTER_HIGHLIGHT_NAMES);
        Self { configuration }
    }
}

impl CodeParser for TreeSitterCodeParser {
    fn highlight(
        &self,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
        cancellation.check()?;
        let mut highlighter = Highlighter::new();
        let events = highlighter
            .highlight(
                &self.configuration,
                source.as_bytes(),
                Some(cancellation.parser_cancellation_flag()),
                |_| None,
            )
            .map_err(tree_sitter_error)?;
        let mut active_highlights = Vec::<usize>::new();
        let mut spans = Vec::<MarkdownCodeSpan>::new();
        for (event_index, event) in events.enumerate() {
            if event_index % 128 == 0 {
                cancellation.check()?;
            }
            match event.map_err(tree_sitter_error)? {
                HighlightEvent::HighlightStart(highlight) => {
                    active_highlights.push(highlight.0);
                }
                HighlightEvent::HighlightEnd => {
                    active_highlights.pop();
                }
                HighlightEvent::Source { start, end } => {
                    let text = source.get(start..end).ok_or_else(|| {
                        PreviewError::Decode(
                            "Tree-sitter returned an invalid UTF-8 range".to_string(),
                        )
                    })?;
                    let highlight_name = active_highlights
                        .last()
                        .and_then(|index| TREE_SITTER_HIGHLIGHT_NAMES.get(*index))
                        .copied();
                    append_tree_sitter_span(&mut spans, text, highlight_name);
                }
            }
        }
        cancellation.check()?;
        Ok(spans)
    }
}

fn tree_sitter_error(error: tree_sitter_highlight::Error) -> PreviewError {
    if error == tree_sitter_highlight::Error::Cancelled {
        PreviewError::Cancelled
    } else {
        PreviewError::Decode(format!("Tree-sitter highlighting failed: {error}"))
    }
}

fn append_tree_sitter_span(
    spans: &mut Vec<MarkdownCodeSpan>,
    text: &str,
    highlight_name: Option<&str>,
) {
    if text.is_empty() {
        return;
    }
    let (light_color, bold, italic) = tree_sitter_style(highlight_name);
    let dark_color = github_dark_color(light_color);
    if let Some(previous) = spans.last_mut()
        && previous.light_color == light_color
        && previous.dark_color == dark_color
        && previous.bold == bold
        && previous.italic == italic
    {
        previous.text.push_str(text);
    } else {
        spans.push(MarkdownCodeSpan {
            text: text.to_string(),
            light_color,
            dark_color,
            bold,
            italic,
        });
    }
}

fn tree_sitter_style(highlight_name: Option<&str>) -> ([u8; 4], bool, bool) {
    let color = match highlight_name.unwrap_or_default() {
        "comment" | "comment.documentation" => [0xff, 0x6a, 0x73, 0x7d],
        "keyword" | "operator" => [0xff, 0xd7, 0x3a, 0x49],
        "string" | "string.escape" | "string.regexp" | "string.special" => [0xff, 0x03, 0x2f, 0x62],
        "number" | "boolean" | "constant" | "constant.builtin" | "escape" => {
            [0xff, 0x00, 0x5c, 0xc5]
        }
        "type" | "type.builtin" | "constructor" | "constructor.builtin" => [0xff, 0x6f, 0x42, 0xc1],
        "function" | "function.builtin" => [0xff, 0x6f, 0x42, 0xc1],
        "attribute" | "module" | "property" | "property.builtin" | "tag" => {
            [0xff, 0x00, 0x5c, 0xc5]
        }
        "variable.builtin" => [0xff, 0xe3, 0x62, 0x09],
        _ => [0xff, 0x24, 0x29, 0x2f],
    };
    (
        color,
        false,
        matches!(highlight_name, Some("comment" | "comment.documentation")),
    )
}

#[derive(Default)]
pub struct JavaScriptParserLoader {
    javascript: OnceLock<Arc<TreeSitterCodeParser>>,
    jsx: OnceLock<Arc<TreeSitterCodeParser>>,
}

impl CodeParserLoader for JavaScriptParserLoader {
    fn id(&self) -> &'static str {
        "javascript"
    }

    fn supports(&self, language: &str) -> bool {
        matches!(
            language,
            "js" | "mjs" | "cjs" | "javascript" | "jsx" | "javascriptreact"
        )
    }

    fn cache_key(&self, language: &str) -> String {
        if matches!(language, "jsx" | "javascriptreact") {
            "jsx"
        } else {
            "javascript"
        }
        .to_string()
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        let jsx = matches!(language, "jsx" | "javascriptreact");
        let slot = if jsx { &self.jsx } else { &self.javascript };
        let parser = load_tree_sitter_parser(slot, cancellation, || build_javascript_parser(jsx))?;
        Ok(parser)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let parser = self.load("javascript", cancellation)?;
        let _ = parser.highlight("const value = 1;\n", cancellation)?;
        let parser = self.load("jsx", cancellation)?;
        let _ = parser.highlight("const view = <main>{value}</main>;\n", cancellation)?;
        Ok(())
    }
}

type TreeSitterParserFactory = fn() -> Result<TreeSitterCodeParser, PreviewError>;

struct StaticTreeSitterParserLoader {
    id: &'static str,
    languages: &'static [&'static str],
    display_name: &'static str,
    sample_source: &'static str,
    build: TreeSitterParserFactory,
    parser: OnceLock<Arc<TreeSitterCodeParser>>,
}

fn static_tree_sitter_loader(
    id: &'static str,
    languages: &'static [&'static str],
    display_name: &'static str,
    sample_source: &'static str,
    build: TreeSitterParserFactory,
) -> StaticTreeSitterParserLoader {
    StaticTreeSitterParserLoader {
        id,
        languages,
        display_name,
        sample_source,
        build,
        parser: OnceLock::new(),
    }
}

impl CodeParserLoader for StaticTreeSitterParserLoader {
    fn id(&self) -> &'static str {
        self.id
    }

    fn supports(&self, language: &str) -> bool {
        self.languages.contains(&language)
    }

    fn cache_key(&self, _language: &str) -> String {
        self.id.to_string()
    }

    fn display_name(&self, _language: &str) -> String {
        self.display_name.to_string()
    }

    fn load(
        &self,
        _language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        let parser = load_tree_sitter_parser(&self.parser, cancellation, self.build)?;
        Ok(parser)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let parser = self.load(self.languages[0], cancellation)?;
        let _ = parser.highlight(self.sample_source, cancellation)?;
        Ok(())
    }
}

#[derive(Default)]
pub struct PlainTextParserLoader {
    parser: OnceLock<Arc<PlainTextCodeParser>>,
}

impl CodeParserLoader for PlainTextParserLoader {
    fn id(&self) -> &'static str {
        "plain-text"
    }

    fn supports(&self, _language: &str) -> bool {
        true
    }

    fn cache_key(&self, _language: &str) -> String {
        self.id().to_string()
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        _language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        cancellation.check()?;
        let parser = self
            .parser
            .get_or_init(|| Arc::new(PlainTextCodeParser))
            .clone();
        cancellation.check()?;
        Ok(parser)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let _ = self.load("", cancellation)?;
        Ok(())
    }
}

struct PlainTextCodeParser;

impl CodeParser for PlainTextCodeParser {
    fn highlight(
        &self,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
        cancellation.check()?;
        if source.is_empty() {
            return Ok(Vec::new());
        }
        let span = MarkdownCodeSpan {
            text: source.to_string(),
            light_color: [0xff, 0x32, 0x32, 0x32],
            dark_color: [0xff, 0xc9, 0xd1, 0xd9],
            bold: false,
            italic: false,
        };
        cancellation.check()?;
        Ok(vec![span])
    }
}

fn load_tree_sitter_parser(
    slot: &OnceLock<Arc<TreeSitterCodeParser>>,
    cancellation: &CancellationToken,
    build: impl FnOnce() -> Result<TreeSitterCodeParser, PreviewError>,
) -> Result<Arc<TreeSitterCodeParser>, PreviewError> {
    cancellation.check()?;
    if let Some(parser) = slot.get() {
        return Ok(parser.clone());
    }
    let parser = Arc::new(build()?);
    cancellation.check()?;
    if slot.set(parser.clone()).is_ok() {
        Ok(parser)
    } else {
        Ok(slot
            .get()
            .expect("Tree-sitter parser initialized by another worker")
            .clone())
    }
}

fn build_typescript_parser(tsx: bool) -> Result<TreeSitterCodeParser, PreviewError> {
    let language = if tsx {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    let mut highlights_query = String::from(tree_sitter_javascript::HIGHLIGHT_QUERY);
    if tsx {
        highlights_query.push('\n');
        highlights_query.push_str(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY);
    }
    highlights_query.push('\n');
    highlights_query.push_str(tree_sitter_typescript::HIGHLIGHTS_QUERY);
    let locals_query = format!(
        "{}\n{}",
        tree_sitter_javascript::LOCALS_QUERY,
        tree_sitter_typescript::LOCALS_QUERY
    );
    let configuration = HighlightConfiguration::new(
        language,
        if tsx { "tsx" } else { "typescript" },
        &highlights_query,
        "",
        &locals_query,
    )
    .map_err(|error| parser_load_error(if tsx { "TSX" } else { "TypeScript" }, error))?;
    Ok(TreeSitterCodeParser::new(configuration))
}

fn build_javascript_parser(jsx: bool) -> Result<TreeSitterCodeParser, PreviewError> {
    let mut highlights_query = String::from(tree_sitter_javascript::HIGHLIGHT_QUERY);
    if jsx {
        highlights_query.push('\n');
        highlights_query.push_str(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY);
    }
    let configuration = HighlightConfiguration::new(
        tree_sitter_javascript::LANGUAGE.into(),
        if jsx { "jsx" } else { "javascript" },
        &highlights_query,
        "",
        tree_sitter_javascript::LOCALS_QUERY,
    )
    .map_err(|error| parser_load_error(if jsx { "JSX" } else { "JavaScript" }, error))?;
    Ok(TreeSitterCodeParser::new(configuration))
}

fn build_rust_parser() -> Result<TreeSitterCodeParser, PreviewError> {
    build_static_tree_sitter_parser(
        "Rust",
        tree_sitter_rust::LANGUAGE.into(),
        tree_sitter_rust::HIGHLIGHTS_QUERY,
    )
}

fn build_shell_parser() -> Result<TreeSitterCodeParser, PreviewError> {
    build_static_tree_sitter_parser(
        "Shell",
        tree_sitter_bash::LANGUAGE.into(),
        tree_sitter_bash::HIGHLIGHT_QUERY,
    )
}

fn build_json_parser() -> Result<TreeSitterCodeParser, PreviewError> {
    build_static_tree_sitter_parser(
        "JSON",
        tree_sitter_json::LANGUAGE.into(),
        tree_sitter_json::HIGHLIGHTS_QUERY,
    )
}

fn build_python_parser() -> Result<TreeSitterCodeParser, PreviewError> {
    build_static_tree_sitter_parser(
        "Python",
        tree_sitter_python::LANGUAGE.into(),
        tree_sitter_python::HIGHLIGHTS_QUERY,
    )
}

fn build_go_parser() -> Result<TreeSitterCodeParser, PreviewError> {
    build_static_tree_sitter_parser(
        "Go",
        tree_sitter_go::LANGUAGE.into(),
        tree_sitter_go::HIGHLIGHTS_QUERY,
    )
}

fn build_static_tree_sitter_parser(
    language_name: &str,
    language: tree_sitter::Language,
    highlights_query: &str,
) -> Result<TreeSitterCodeParser, PreviewError> {
    let configuration =
        HighlightConfiguration::new(language, language_name, highlights_query, "", "")
            .map_err(|error| parser_load_error(language_name, error))?;
    Ok(TreeSitterCodeParser::new(configuration))
}

fn parser_load_error(language: &str, error: impl std::fmt::Display) -> PreviewError {
    PreviewError::Decode(format!("{language} parser load failed: {error}"))
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct PlainParser;

    impl CodeParser for PlainParser {
        fn highlight(
            &self,
            source: &str,
            cancellation: &CancellationToken,
        ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
            cancellation.check()?;
            Ok(vec![MarkdownCodeSpan {
                text: source.to_string(),
                light_color: [255, 50, 50, 50],
                dark_color: [255, 201, 209, 217],
                bold: false,
                italic: false,
            }])
        }
    }

    struct CountingLoader {
        id: &'static str,
        language: &'static str,
        loads: Arc<AtomicUsize>,
        preloads: Arc<AtomicUsize>,
        cancel_during_load: bool,
    }

    impl CodeParserLoader for CountingLoader {
        fn id(&self) -> &'static str {
            self.id
        }

        fn supports(&self, language: &str) -> bool {
            language == self.language
        }

        fn display_name(&self, language: &str) -> String {
            language_display_name(language)
        }

        fn load(
            &self,
            _language: &str,
            cancellation: &CancellationToken,
        ) -> Result<Arc<dyn CodeParser>, PreviewError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            if self.cancel_during_load {
                cancellation.cancel();
            }
            Ok(Arc::new(PlainParser))
        }

        fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
            cancellation.check()?;
            self.preloads.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    struct TaggedParser {
        color: [u8; 4],
    }

    impl CodeParser for TaggedParser {
        fn highlight(
            &self,
            source: &str,
            cancellation: &CancellationToken,
        ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
            cancellation.check()?;
            Ok(vec![MarkdownCodeSpan {
                text: source.to_string(),
                light_color: self.color,
                dark_color: self.color,
                bold: false,
                italic: false,
            }])
        }
    }

    struct CollidingCacheKeyLoader {
        id: &'static str,
        language: &'static str,
        color: [u8; 4],
    }

    impl CodeParserLoader for CollidingCacheKeyLoader {
        fn id(&self) -> &'static str {
            self.id
        }

        fn supports(&self, language: &str) -> bool {
            language == self.language
        }

        fn cache_key(&self, _language: &str) -> String {
            "shared".to_string()
        }

        fn display_name(&self, language: &str) -> String {
            language_display_name(language)
        }

        fn load(
            &self,
            _language: &str,
            cancellation: &CancellationToken,
        ) -> Result<Arc<dyn CodeParser>, PreviewError> {
            cancellation.check()?;
            Ok(Arc::new(TaggedParser { color: self.color }))
        }

        fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
            cancellation.check()
        }
    }

    #[test]
    fn on_demand_initializes_only_the_selected_loader_and_caches_it() {
        let ts_loads = Arc::new(AtomicUsize::new(0));
        let rust_loads = Arc::new(AtomicUsize::new(0));
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::OnDemand);
        registry.register(CountingLoader {
            id: "ts",
            language: "typescript",
            loads: ts_loads.clone(),
            preloads: Arc::new(AtomicUsize::new(0)),
            cancel_during_load: false,
        });
        registry.register(CountingLoader {
            id: "rust",
            language: "rust",
            loads: rust_loads.clone(),
            preloads: Arc::new(AtomicUsize::new(0)),
            cancel_during_load: false,
        });
        let cancellation = CancellationToken::default();

        registry
            .highlight("const value = 1;", "typescript", &cancellation)
            .unwrap();
        registry
            .highlight("const value = 2;", "typescript", &cancellation)
            .unwrap();

        assert_eq!(ts_loads.load(Ordering::Relaxed), 1);
        assert_eq!(rust_loads.load(Ordering::Relaxed), 0);
        assert_eq!(registry.loaded_parser_count(), 1);
    }

    #[test]
    fn custom_loaders_receive_trimmed_lowercase_language_without_builtin_alias_rewriting() {
        let loads = Arc::new(AtomicUsize::new(0));
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::OnDemand);
        registry.register(CountingLoader {
            id: "custom-ts",
            language: "ts",
            loads: loads.clone(),
            preloads: Arc::new(AtomicUsize::new(0)),
            cancel_during_load: false,
        });

        registry
            .highlight("const value = 1;", " TS ", &CancellationToken::default())
            .unwrap();

        assert_eq!(loads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parser_cache_keys_are_namespaced_by_loader_id() {
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::OnDemand);
        registry.register(CollidingCacheKeyLoader {
            id: "first",
            language: "first-language",
            color: [255, 1, 2, 3],
        });
        registry.register(CollidingCacheKeyLoader {
            id: "second",
            language: "second-language",
            color: [255, 4, 5, 6],
        });
        let cancellation = CancellationToken::default();

        let first = registry
            .highlight("first", "first-language", &cancellation)
            .unwrap();
        let second = registry
            .highlight("second", "second-language", &cancellation)
            .unwrap();

        assert_eq!(first[0].light_color, [255, 1, 2, 3]);
        assert_eq!(second[0].light_color, [255, 4, 5, 6]);
        assert_eq!(registry.loaded_parser_count(), 2);
    }

    #[test]
    fn on_demand_reports_loading_and_ready_only_for_a_cache_miss() {
        let loads = Arc::new(AtomicUsize::new(0));
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::OnDemand);
        registry.register(CountingLoader {
            id: "ts",
            language: "typescript",
            loads: loads.clone(),
            preloads: Arc::new(AtomicUsize::new(0)),
            cancel_during_load: false,
        });
        let cancellation = CancellationToken::default();
        let progress = Arc::new(Mutex::new(Vec::new()));
        let observed = progress.clone();
        cancellation.set_progress_handler(move |event| {
            observed.lock().expect("progress lock poisoned").push(event);
        });

        registry
            .highlight("const value = 1;", "typescript", &cancellation)
            .unwrap();
        registry
            .highlight("const value = 2;", "typescript", &cancellation)
            .unwrap();

        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(
            *progress.lock().expect("progress lock poisoned"),
            vec![
                PreviewPreparationProgress::ParserLoading {
                    language: "typescript".to_string(),
                    loader_id: "ts",
                    display_name: "TypeScript".to_string(),
                },
                PreviewPreparationProgress::ParserReady {
                    language: "typescript".to_string(),
                    loader_id: "ts",
                    display_name: "TypeScript".to_string(),
                },
            ]
        );
    }

    #[test]
    fn preload_all_initializes_every_registered_loader_once() {
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::PreloadAll);
        registry.register(CountingLoader {
            id: "first",
            language: "first",
            loads: Arc::new(AtomicUsize::new(0)),
            preloads: first.clone(),
            cancel_during_load: false,
        });
        registry.register(CountingLoader {
            id: "second",
            language: "second",
            loads: Arc::new(AtomicUsize::new(0)),
            preloads: second.clone(),
            cancel_during_load: false,
        });
        let cancellation = CancellationToken::default();

        registry.prepare(&cancellation).unwrap();
        registry.prepare(&cancellation).unwrap();

        assert_eq!(first.load(Ordering::Relaxed), 1);
        assert_eq!(second.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancellation_prevents_late_parser_cache_insertion() {
        let loads = Arc::new(AtomicUsize::new(0));
        let mut registry = CodeParserRegistry::new(ParserLoadStrategy::OnDemand);
        registry.register(CountingLoader {
            id: "cancel",
            language: "typescript",
            loads: loads.clone(),
            preloads: Arc::new(AtomicUsize::new(0)),
            cancel_during_load: true,
        });
        let cancellation = CancellationToken::default();
        let progress = Arc::new(Mutex::new(Vec::new()));
        let observed = progress.clone();
        cancellation.set_progress_handler(move |event| {
            observed.lock().expect("progress lock poisoned").push(event);
        });

        assert_eq!(
            registry
                .highlight("const value = 1;", "typescript", &cancellation)
                .unwrap_err(),
            PreviewError::Cancelled
        );
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(registry.loaded_parser_count(), 0);
        assert_eq!(
            *progress.lock().expect("progress lock poisoned"),
            vec![PreviewPreparationProgress::ParserLoading {
                language: "typescript".to_string(),
                loader_id: "cancel",
                display_name: "TypeScript".to_string(),
            }]
        );
    }

    #[test]
    fn tree_sitter_languages_preserve_source_and_emit_multiple_token_styles() {
        let registry = CodeParserRegistry::with_builtins(ParserLoadStrategy::OnDemand);
        let cancellation = CancellationToken::default();
        for (language, source) in [
            (
                "rust",
                "pub fn ready(name: &str) -> bool {\r\n    // UTF-8 stays intact.\r\n    name == \"世界\"\r\n}\r\n",
            ),
            (
                "bash",
                "set -euo pipefail\nname='Usher'\nprintf '%s\\n' \"$name\" # ready\n",
            ),
            (
                "javascript",
                "export const count = 42; // ready\nconst label = 'Usher';\n",
            ),
            (
                "jsx",
                "export const view = <main title=\"Usher\">{42}</main>;\n",
            ),
            (
                "typescript",
                "export const count: number = 42; // ready\nconst label = 'Usher';\n",
            ),
            (
                "tsx",
                "export const view = <main title=\"Usher\">{42}</main>;\n",
            ),
            (
                "json",
                "{\"ready\": true, \"count\": 42, \"value\": null}\n",
            ),
            (
                "python",
                "def ready(value: int) -> bool:\n    # Keep comments styled.\n    return value > 0\n",
            ),
            (
                "go",
                "package main\n\nfunc ready(value int) bool { return value > 0 }\n",
            ),
        ] {
            let spans = registry.highlight(source, language, &cancellation).unwrap();
            assert_eq!(
                spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>(),
                source
            );
            assert!(
                spans
                    .iter()
                    .map(|span| span.light_color)
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    >= 3,
                "{language} produced too few token styles: {spans:?}"
            );
        }
    }

    #[test]
    fn common_language_aliases_route_to_dedicated_tree_sitter_loaders() {
        let registry = CodeParserRegistry::with_builtins(ParserLoadStrategy::OnDemand);

        for alias in ["ts", "typescript", "tsx", "typescriptreact"] {
            assert_eq!(registry.loader_id_for(alias), Some("typescript"));
        }
        for alias in ["js", "javascript", "jsx", "mjs", "cjs"] {
            assert_eq!(registry.loader_id_for(alias), Some("javascript"));
        }
        for alias in ["rs", "rust"] {
            assert_eq!(registry.loader_id_for(alias), Some("rust"));
        }
        for alias in ["sh", "bash", "shell", "shellscript", "zsh"] {
            assert_eq!(registry.loader_id_for(alias), Some("shell"));
        }
        for alias in ["py", "python", "python3"] {
            assert_eq!(registry.loader_id_for(alias), Some("python"));
        }
        for alias in ["go", "golang"] {
            assert_eq!(registry.loader_id_for(alias), Some("go"));
        }
        assert_eq!(registry.loader_id_for("json"), Some("json"));
        for unsupported in ["", "toml", "slint", "usher-unknown"] {
            assert_eq!(registry.loader_id_for(unsupported), Some("plain-text"));
        }
    }

    #[test]
    fn aliases_and_unsupported_languages_reuse_canonical_parsers() {
        let registry = CodeParserRegistry::with_builtins(ParserLoadStrategy::OnDemand);
        let cancellation = CancellationToken::default();

        registry
            .highlight("fn main() {}\n", "rust", &cancellation)
            .unwrap();
        registry
            .highlight("fn alias() {}\n", "rs", &cancellation)
            .unwrap();
        assert_eq!(registry.loaded_parser_count(), 1);

        for (language, source) in [
            ("toml", "ready = true\n"),
            ("usher-unknown", "alpha < beta\n"),
        ] {
            let spans = registry.highlight(source, language, &cancellation).unwrap();
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, source);
            assert_eq!(spans[0].light_color, [0xff, 0x32, 0x32, 0x32]);
            assert_eq!(spans[0].dark_color, [0xff, 0xc9, 0xd1, 0xd9]);
        }
        assert_eq!(registry.loaded_parser_count(), 2);
    }
}
