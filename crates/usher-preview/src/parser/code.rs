use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
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
    loaded: Mutex<HashMap<String, Arc<dyn CodeParser>>>,
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
        registry.register(SyntectParserLoader::default());
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
        let parser = {
            let mut loaded = self.loaded.lock().expect("code parser cache lock poisoned");
            if let Some(parser) = loaded.get(&language) {
                parser.clone()
            } else {
                let loader = self
                    .loaders
                    .iter()
                    .find(|loader| loader.supports(&language))
                    .ok_or_else(|| {
                        PreviewError::Decode(format!(
                            "no code parser loader accepts language '{language}'"
                        ))
                    })?;
                let loader_id = loader.id();
                let display_name = loader.display_name(&language);
                cancellation.report_progress(PreviewPreparationProgress::ParserLoading {
                    language: language.clone(),
                    loader_id,
                    display_name: display_name.clone(),
                });
                let parser = loader.load(&language, cancellation)?;
                cancellation.check()?;
                loaded.insert(language.clone(), parser.clone());
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
        "js" | "javascript" => "JavaScript".to_string(),
        "jsx" | "javascriptreact" => "JSX".to_string(),
        "ts" | "typescript" => "TypeScript".to_string(),
        "tsx" | "typescriptreact" => "TSX".to_string(),
        "rs" | "rust" => "Rust".to_string(),
        "json" => "JSON".to_string(),
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

struct SyntectResources {
    syntaxes: SyntaxSet,
    light_theme: Theme,
}

#[derive(Default)]
struct SyntectRepository {
    resources: OnceLock<SyntectResources>,
}

impl SyntectRepository {
    fn resources(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<&SyntectResources, PreviewError> {
        cancellation.check()?;
        let resources = self.resources.get_or_init(|| {
            let mut themes = ThemeSet::load_defaults().themes;
            SyntectResources {
                syntaxes: two_face::syntax::extra_newlines(),
                light_theme: themes
                    .remove("InspiredGitHub")
                    .expect("syntect must bundle the InspiredGitHub theme"),
            }
        });
        cancellation.check()?;
        Ok(resources)
    }

    fn parser(
        self: &Arc<Self>,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        let resources = self.resources(cancellation)?;
        let syntax = resolve_code_syntax(&resources.syntaxes, language);
        Ok(Arc::new(SyntectCodeParser {
            repository: self.clone(),
            syntax_name: syntax.name.clone(),
        }))
    }

    fn preload_all(self: &Arc<Self>, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let syntax_names = self
            .resources(cancellation)?
            .syntaxes
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.clone())
            .collect::<Vec<_>>();
        for syntax_name in syntax_names {
            cancellation.check()?;
            let parser = self.parser(&syntax_name, cancellation)?;
            let _ = parser.highlight("\n", cancellation)?;
        }
        Ok(())
    }
}

static SHARED_SYNTECT_REPOSITORY: LazyLock<Arc<SyntectRepository>> =
    LazyLock::new(|| Arc::new(SyntectRepository::default()));

struct SyntectCodeParser {
    repository: Arc<SyntectRepository>,
    syntax_name: String,
}

impl CodeParser for SyntectCodeParser {
    fn highlight(
        &self,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
        let resources = self.repository.resources(cancellation)?;
        let syntax = resources
            .syntaxes
            .find_syntax_by_name(&self.syntax_name)
            .unwrap_or_else(|| resources.syntaxes.find_syntax_plain_text());
        highlight_source(source, syntax, resources, cancellation)
    }
}

#[derive(Default)]
pub struct TsParserLoader {
    typescript: OnceLock<Arc<TreeSitterTypeScriptParser>>,
    tsx: OnceLock<Arc<TreeSitterTypeScriptParser>>,
}

impl CodeParserLoader for TsParserLoader {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn supports(&self, language: &str) -> bool {
        matches!(language, "ts" | "tsx" | "typescript" | "typescriptreact")
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        cancellation.check()?;
        let tsx = matches!(language, "tsx" | "typescriptreact");
        let slot = if tsx { &self.tsx } else { &self.typescript };
        if let Some(parser) = slot.get() {
            return Ok(parser.clone());
        }
        let parser = Arc::new(TreeSitterTypeScriptParser::new(tsx)?);
        cancellation.check()?;
        if slot.set(parser.clone()).is_ok() {
            Ok(parser)
        } else {
            Ok(slot
                .get()
                .expect("TypeScript parser initialized by another worker")
                .clone())
        }
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
    "constant",
    "constant.builtin",
    "constructor",
    "escape",
    "function",
    "function.builtin",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.escape",
    "string.regexp",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

struct TreeSitterTypeScriptParser {
    configuration: HighlightConfiguration,
}

impl TreeSitterTypeScriptParser {
    fn new(tsx: bool) -> Result<Self, PreviewError> {
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
        let mut configuration = HighlightConfiguration::new(
            language,
            if tsx { "tsx" } else { "typescript" },
            &highlights_query,
            "",
            &locals_query,
        )
        .map_err(|error| PreviewError::Decode(format!("TypeScript parser load failed: {error}")))?;
        configuration.configure(TREE_SITTER_HIGHLIGHT_NAMES);
        Ok(Self { configuration })
    }
}

impl CodeParser for TreeSitterTypeScriptParser {
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
                            "TypeScript parser returned an invalid UTF-8 range".to_string(),
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
        PreviewError::Decode(format!("TypeScript highlighting failed: {error}"))
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
        "comment" => [0xff, 0x6a, 0x73, 0x7d],
        "keyword" | "operator" => [0xff, 0xd7, 0x3a, 0x49],
        "string" | "string.escape" | "string.regexp" => [0xff, 0x03, 0x2f, 0x62],
        "number" | "boolean" | "constant" | "constant.builtin" | "escape" => {
            [0xff, 0x00, 0x5c, 0xc5]
        }
        "type" | "type.builtin" | "constructor" => [0xff, 0x6f, 0x42, 0xc1],
        "function" | "function.builtin" => [0xff, 0x6f, 0x42, 0xc1],
        "attribute" | "module" | "property" | "property.builtin" | "tag" => {
            [0xff, 0x00, 0x5c, 0xc5]
        }
        "variable.builtin" => [0xff, 0xe3, 0x62, 0x09],
        _ => [0xff, 0x24, 0x29, 0x2f],
    };
    (color, false, highlight_name == Some("comment"))
}

#[derive(Clone)]
pub struct JavaScriptParserLoader {
    repository: Arc<SyntectRepository>,
}

impl Default for JavaScriptParserLoader {
    fn default() -> Self {
        Self {
            repository: SHARED_SYNTECT_REPOSITORY.clone(),
        }
    }
}

impl CodeParserLoader for JavaScriptParserLoader {
    fn id(&self) -> &'static str {
        "javascript"
    }

    fn supports(&self, language: &str) -> bool {
        matches!(
            language,
            "js" | "jsx" | "mjs" | "cjs" | "javascript" | "javascriptreact"
        )
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        let syntax = if matches!(language, "jsx" | "javascriptreact") {
            "JavaScript (Babel)"
        } else {
            "JavaScript"
        };
        self.repository.parser(syntax, cancellation)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        let parser = self.load("javascript", cancellation)?;
        let _ = parser.highlight("const value = 1;\n", cancellation)?;
        let parser = self.load("jsx", cancellation)?;
        let _ = parser.highlight("const view = <main>{value}</main>;\n", cancellation)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct SyntectParserLoader {
    repository: Arc<SyntectRepository>,
}

impl Default for SyntectParserLoader {
    fn default() -> Self {
        Self {
            repository: SHARED_SYNTECT_REPOSITORY.clone(),
        }
    }
}

impl CodeParserLoader for SyntectParserLoader {
    fn id(&self) -> &'static str {
        "syntect-language"
    }

    fn supports(&self, _language: &str) -> bool {
        true
    }

    fn display_name(&self, language: &str) -> String {
        language_display_name(language)
    }

    fn load(
        &self,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CodeParser>, PreviewError> {
        self.repository.parser(language, cancellation)
    }

    fn preload(&self, cancellation: &CancellationToken) -> Result<(), PreviewError> {
        self.repository.preload_all(cancellation)
    }
}

fn highlight_source(
    source: &str,
    syntax: &syntect::parsing::SyntaxReference,
    resources: &SyntectResources,
    cancellation: &CancellationToken,
) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
    let mut highlighter = HighlightLines::new(syntax, &resources.light_theme);
    let mut spans = Vec::<MarkdownCodeSpan>::new();
    for (line_index, line) in source.split_inclusive('\n').enumerate() {
        if line_index % 64 == 0 {
            cancellation.check()?;
        }
        let ranges = highlighter
            .highlight_line(line, &resources.syntaxes)
            .unwrap_or_else(|_| vec![(syntect::highlighting::Style::default(), line)]);
        for (style, text) in ranges {
            let light_color = [
                style.foreground.a,
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            ];
            let dark_color = github_dark_color(light_color);
            let bold = style.font_style.contains(FontStyle::BOLD);
            let italic = style.font_style.contains(FontStyle::ITALIC);
            if let Some(previous) = spans.last_mut()
                && previous.light_color == light_color
                && previous.dark_color == dark_color
                && previous.bold == bold
                && previous.italic == italic
            {
                previous.text.push_str(text);
            } else if !text.is_empty() {
                spans.push(MarkdownCodeSpan {
                    text: text.to_string(),
                    light_color,
                    dark_color,
                    bold,
                    italic,
                });
            }
        }
    }
    Ok(spans)
}

fn resolve_code_syntax<'a>(
    syntaxes: &'a SyntaxSet,
    language: &str,
) -> &'a syntect::parsing::SyntaxReference {
    let language = language.trim();
    if language.is_empty() {
        return syntaxes.find_syntax_plain_text();
    }

    syntaxes
        .find_syntax_by_token(language)
        .or_else(|| syntaxes.find_syntax_by_extension(language))
        .or_else(|| {
            syntaxes
                .syntaxes()
                .iter()
                .find(|syntax| syntax.name.eq_ignore_ascii_case(language))
        })
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
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
    fn typescript_and_tsx_preserve_source_and_emit_multiple_token_styles() {
        let registry = CodeParserRegistry::with_builtins(ParserLoadStrategy::OnDemand);
        let cancellation = CancellationToken::default();
        for (language, source) in [
            (
                "typescript",
                "export const count: number = 42; // ready\nconst label = 'Usher';\n",
            ),
            (
                "tsx",
                "export const view = <main title=\"Usher\">{42}</main>;\n",
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
    fn typescript_and_javascript_aliases_route_to_dedicated_loaders() {
        let registry = CodeParserRegistry::with_builtins(ParserLoadStrategy::OnDemand);

        for alias in ["ts", "typescript", "tsx", "typescriptreact"] {
            assert_eq!(registry.loader_id_for(alias), Some("typescript"));
        }
        for alias in ["js", "javascript", "jsx", "mjs", "cjs"] {
            assert_eq!(registry.loader_id_for(alias), Some("javascript"));
        }
        assert_eq!(registry.loader_id_for("rust"), Some("syntect-language"));
    }
}
