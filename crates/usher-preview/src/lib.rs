//! Framework-neutral artifact preview providers and registry.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "markdown")]
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
#[cfg(feature = "markdown")]
use std::sync::LazyLock;
#[cfg(feature = "markdown")]
use syntect::easy::HighlightLines;
#[cfg(feature = "markdown")]
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
#[cfg(feature = "markdown")]
use syntect::parsing::SyntaxSet;

pub const MARKDOWN_PROVIDER_ID: &str = "markdown";
pub const IMAGE_PROVIDER_ID: &str = "image";
pub const PLAIN_TEXT_PROVIDER_ID: &str = "plain-text";
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DECODED_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactHint {
    Auto,
    Markdown,
    Image,
    PlainText,
}

#[derive(Debug, Clone)]
pub enum ArtifactBody {
    Text(String),
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ArtifactSource {
    pub id: String,
    pub title: String,
    pub mime_type: Option<String>,
    pub hint: ArtifactHint,
    pub body: ArtifactBody,
}

impl ArtifactSource {
    pub fn inline_markdown(
        id: impl Into<String>,
        title: impl Into<String>,
        markdown: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            mime_type: Some("text/markdown".into()),
            hint: ArtifactHint::Markdown,
            body: ArtifactBody::Text(markdown.into()),
        }
    }

    pub fn file(path: impl Into<PathBuf>, mime_type: Option<String>) -> Self {
        let path = path.into();
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Preview")
            .to_string();
        Self {
            id: path.to_string_lossy().into_owned(),
            title,
            mime_type,
            hint: ArtifactHint::Auto,
            body: ArtifactBody::File(path),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewCapabilities {
    pub copy: bool,
    pub search: bool,
    pub zoom: bool,
    pub open_external: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownBlockKind {
    Heading1,
    Heading2,
    Heading3,
    Paragraph,
    ListItem,
    Quote,
    CodeBlock,
    Rule,
    TableRow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownInlineKind {
    Text,
    Strong,
    Emphasis,
    Code,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownInlineSpan {
    pub kind: MarkdownInlineKind,
    pub text: String,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlock {
    pub kind: MarkdownBlockKind,
    pub text: String,
    pub spans: Vec<MarkdownInlineSpan>,
    pub code_language: String,
    pub code_spans: Vec<MarkdownCodeSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownCodeSpan {
    pub text: String,
    pub light_color: [u8; 4],
    pub dark_color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocument {
    pub title: String,
    pub plain_text: String,
    pub blocks: Vec<MarkdownBlock>,
}

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum PreparedContent {
    Markdown(MarkdownDocument),
    Image(DecodedImage),
    PlainText(String),
}

#[derive(Debug, Clone)]
pub struct PreparedArtifact {
    pub id: String,
    pub title: String,
    pub provider_id: &'static str,
    pub source_path: Option<PathBuf>,
    pub capabilities: PreviewCapabilities,
    pub content: PreparedContent,
    pub approximate_bytes: usize,
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn check(&self) -> Result<(), PreviewError> {
        if self.is_cancelled() {
            Err(PreviewError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewError {
    Cancelled,
    Unsupported,
    SourceTooLarge { bytes: u64, limit: usize },
    InvalidUtf8,
    Io(String),
    Decode(String),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("preview preparation cancelled"),
            Self::Unsupported => f.write_str("no enabled preview provider supports this artifact"),
            Self::SourceTooLarge { bytes, limit } => {
                write!(f, "artifact is too large ({bytes} bytes; limit {limit})")
            }
            Self::InvalidUtf8 => f.write_str("artifact is not valid UTF-8 text"),
            Self::Io(message) => write!(f, "artifact I/O failed: {message}"),
            Self::Decode(message) => write!(f, "artifact decode failed: {message}"),
        }
    }
}

impl std::error::Error for PreviewError {}

pub struct MaterializedSource {
    id: String,
    title: String,
    mime_type: Option<String>,
    hint: ArtifactHint,
    path: Option<PathBuf>,
    bytes: Vec<u8>,
}

impl MaterializedSource {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    pub fn hint(&self) -> ArtifactHint {
        self.hint
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn extension(&self) -> Option<&str> {
        self.path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|value| value.to_str())
    }

    pub fn text(&self) -> Result<&str, PreviewError> {
        std::str::from_utf8(&self.bytes).map_err(|_| PreviewError::InvalidUtf8)
    }
}

pub trait PreviewProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn probe(&self, source: &MaterializedSource) -> u16;
    fn prepare(
        &self,
        source: &MaterializedSource,
        cancellation: &CancellationToken,
    ) -> Result<PreparedArtifact, PreviewError>;
}

#[derive(Clone)]
pub struct PreviewRegistry {
    providers: Vec<Arc<dyn PreviewProvider>>,
    enabled: HashMap<&'static str, bool>,
}

impl Default for PreviewRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl PreviewRegistry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            enabled: HashMap::new(),
        }
    }

    pub fn with_builtins() -> Self {
        #[cfg(not(any(feature = "markdown", feature = "image", feature = "plain-text")))]
        return Self::new();

        #[cfg(any(feature = "markdown", feature = "image", feature = "plain-text"))]
        let mut registry = Self::new();
        #[cfg(feature = "markdown")]
        registry.register(MarkdownProvider);
        #[cfg(feature = "image")]
        registry.register(ImageProvider);
        #[cfg(feature = "plain-text")]
        registry.register(PlainTextProvider);
        #[cfg(any(feature = "markdown", feature = "image", feature = "plain-text"))]
        registry
    }

    pub fn register(&mut self, provider: impl PreviewProvider + 'static) {
        let id = provider.id();
        self.providers.retain(|existing| existing.id() != id);
        self.enabled.entry(id).or_insert(true);
        self.providers.push(Arc::new(provider));
    }

    pub fn provider_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.providers.iter().map(|provider| provider.id())
    }

    pub fn set_enabled(&mut self, provider_id: &str, enabled: bool) -> bool {
        let Some(id) = self
            .providers
            .iter()
            .map(|provider| provider.id())
            .find(|id| *id == provider_id)
        else {
            return false;
        };
        self.enabled.insert(id, enabled);
        true
    }

    pub fn is_enabled(&self, provider_id: &str) -> bool {
        self.enabled.get(provider_id).copied().unwrap_or(false)
    }

    pub fn prepare(
        &self,
        source: ArtifactSource,
        cancellation: &CancellationToken,
    ) -> Result<PreparedArtifact, PreviewError> {
        cancellation.check()?;
        let source = materialize(source, cancellation)?;
        cancellation.check()?;

        let provider = self
            .providers
            .iter()
            .enumerate()
            .filter(|(_, provider)| self.is_enabled(provider.id()))
            .map(|(index, provider)| (provider.probe(&source), index, provider))
            .filter(|(score, _, _)| *score > 0)
            .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
            .map(|(_, _, provider)| provider)
            .ok_or(PreviewError::Unsupported)?;

        provider.prepare(&source, cancellation)
    }
}

fn materialize(
    source: ArtifactSource,
    cancellation: &CancellationToken,
) -> Result<MaterializedSource, PreviewError> {
    let (path, bytes) = match source.body {
        ArtifactBody::Text(text) => (None, text.into_bytes()),
        ArtifactBody::Bytes(bytes) => (None, bytes),
        ArtifactBody::File(path) => {
            let metadata =
                fs::metadata(&path).map_err(|error| PreviewError::Io(error.to_string()))?;
            if metadata.len() > MAX_SOURCE_BYTES as u64 {
                return Err(PreviewError::SourceTooLarge {
                    bytes: metadata.len(),
                    limit: MAX_SOURCE_BYTES,
                });
            }
            cancellation.check()?;
            let bytes = fs::read(&path).map_err(|error| PreviewError::Io(error.to_string()))?;
            (Some(path), bytes)
        }
    };
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(PreviewError::SourceTooLarge {
            bytes: bytes.len() as u64,
            limit: MAX_SOURCE_BYTES,
        });
    }
    Ok(MaterializedSource {
        id: source.id,
        title: source.title,
        mime_type: source.mime_type.map(|mime| mime.to_ascii_lowercase()),
        hint: source.hint,
        path,
        bytes,
    })
}

#[cfg(feature = "markdown")]
struct MarkdownProvider;

#[cfg(feature = "markdown")]
impl PreviewProvider for MarkdownProvider {
    fn id(&self) -> &'static str {
        MARKDOWN_PROVIDER_ID
    }

    fn label(&self) -> &'static str {
        "Markdown"
    }

    fn probe(&self, source: &MaterializedSource) -> u16 {
        if source.hint == ArtifactHint::Markdown {
            return 120;
        }
        if source.mime_type.as_deref() == Some("text/markdown") {
            return 110;
        }
        if matches!(source.extension(), Some("md" | "markdown" | "mdown")) {
            return 100;
        }
        0
    }

    fn prepare(
        &self,
        source: &MaterializedSource,
        cancellation: &CancellationToken,
    ) -> Result<PreparedArtifact, PreviewError> {
        cancellation.check()?;
        let markdown = source.text()?;
        let document = parse_markdown_inner(markdown, &source.title, Some(cancellation))?;
        cancellation.check()?;
        let approximate_bytes = markdown.len()
            + document
                .blocks
                .iter()
                .map(|block| {
                    block.text.len()
                        + block
                            .spans
                            .iter()
                            .map(|span| span.text.len() + span.href.len() + 16)
                            .sum::<usize>()
                        + block.code_language.len()
                        + block
                            .code_spans
                            .iter()
                            .map(|span| span.text.len() + 24)
                            .sum::<usize>()
                        + 32
                })
                .sum::<usize>();
        Ok(PreparedArtifact {
            id: source.id.clone(),
            title: document.title.clone(),
            provider_id: MARKDOWN_PROVIDER_ID,
            source_path: source.path.clone(),
            capabilities: PreviewCapabilities {
                copy: true,
                search: true,
                zoom: false,
                open_external: source.path.is_some(),
            },
            content: PreparedContent::Markdown(document),
            approximate_bytes,
        })
    }
}

#[cfg(feature = "image")]
struct ImageProvider;

#[cfg(feature = "image")]
impl PreviewProvider for ImageProvider {
    fn id(&self) -> &'static str {
        IMAGE_PROVIDER_ID
    }

    fn label(&self) -> &'static str {
        "Image"
    }

    fn probe(&self, source: &MaterializedSource) -> u16 {
        if source.hint == ArtifactHint::Image {
            return 120;
        }
        if image::guess_format(&source.bytes).is_ok() {
            return 115;
        }
        if source
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            return 90;
        }
        if matches!(
            source.extension(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico")
        ) {
            return 70;
        }
        0
    }

    fn prepare(
        &self,
        source: &MaterializedSource,
        cancellation: &CancellationToken,
    ) -> Result<PreparedArtifact, PreviewError> {
        use image::ImageReader;
        use std::io::Cursor;

        cancellation.check()?;
        let reader = ImageReader::new(Cursor::new(source.bytes.as_slice()))
            .with_guessed_format()
            .map_err(|error| PreviewError::Decode(error.to_string()))?;
        let (width, height) = reader
            .into_dimensions()
            .map_err(|error| PreviewError::Decode(error.to_string()))?;
        let decoded_bytes = width as usize * height as usize * 4;
        if decoded_bytes > MAX_DECODED_IMAGE_BYTES {
            return Err(PreviewError::SourceTooLarge {
                bytes: decoded_bytes as u64,
                limit: MAX_DECODED_IMAGE_BYTES,
            });
        }
        cancellation.check()?;
        let decoded = image::load_from_memory(&source.bytes)
            .map_err(|error| PreviewError::Decode(error.to_string()))?
            .into_rgba8();
        cancellation.check()?;
        let rgba = decoded.into_raw();
        Ok(PreparedArtifact {
            id: source.id.clone(),
            title: source.title.clone(),
            provider_id: IMAGE_PROVIDER_ID,
            source_path: source.path.clone(),
            capabilities: PreviewCapabilities {
                copy: false,
                search: false,
                zoom: true,
                open_external: source.path.is_some(),
            },
            approximate_bytes: rgba.len(),
            content: PreparedContent::Image(DecodedImage {
                width,
                height,
                rgba,
            }),
        })
    }
}

#[cfg(feature = "plain-text")]
struct PlainTextProvider;

#[cfg(feature = "plain-text")]
impl PreviewProvider for PlainTextProvider {
    fn id(&self) -> &'static str {
        PLAIN_TEXT_PROVIDER_ID
    }

    fn label(&self) -> &'static str {
        "Plain text"
    }

    fn probe(&self, source: &MaterializedSource) -> u16 {
        if !matches!(source.hint, ArtifactHint::Auto | ArtifactHint::PlainText) {
            return 0;
        }
        if source.hint == ArtifactHint::PlainText {
            return 80;
        }
        if source.mime_type.as_deref() == Some("text/markdown") {
            return 0;
        }
        if source
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("text/"))
        {
            return 50;
        }
        if std::str::from_utf8(&source.bytes).is_ok() {
            return 10;
        }
        0
    }

    fn prepare(
        &self,
        source: &MaterializedSource,
        cancellation: &CancellationToken,
    ) -> Result<PreparedArtifact, PreviewError> {
        cancellation.check()?;
        let text = source.text()?.to_string();
        Ok(PreparedArtifact {
            id: source.id.clone(),
            title: source.title.clone(),
            provider_id: PLAIN_TEXT_PROVIDER_ID,
            source_path: source.path.clone(),
            capabilities: PreviewCapabilities {
                copy: true,
                search: true,
                zoom: false,
                open_external: source.path.is_some(),
            },
            approximate_bytes: text.len(),
            content: PreparedContent::PlainText(text),
        })
    }
}

#[cfg(feature = "markdown")]
#[derive(Clone)]
struct PendingBlock {
    kind: MarkdownBlockKind,
    text: String,
    spans: Vec<MarkdownInlineSpan>,
    code_language: String,
}

#[cfg(feature = "markdown")]
pub fn parse_markdown(markdown: &str, fallback_title: &str) -> MarkdownDocument {
    parse_markdown_inner(markdown, fallback_title, None)
        .expect("non-cancellable Markdown parsing cannot be cancelled")
}

#[cfg(feature = "markdown")]
fn parse_markdown_inner(
    markdown: &str,
    fallback_title: &str,
    cancellation: Option<&CancellationToken>,
) -> Result<MarkdownDocument, PreviewError> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut current: Option<PendingBlock> = None;
    let mut list_depth = 0usize;
    let mut strong = false;
    let mut emphasis = false;
    let mut link_href: Option<String> = None;
    let mut in_image = false;

    let finalize = |blocks: &mut Vec<MarkdownBlock>, current: &mut Option<PendingBlock>| {
        if let Some(pending) = current.take() {
            let text = pending.text.trim_end().to_string();
            if !text.trim().is_empty() {
                blocks.push(MarkdownBlock {
                    kind: pending.kind,
                    text,
                    spans: pending.spans,
                    code_language: pending.code_language,
                    code_spans: Vec::new(),
                });
            }
        }
    };

    for (event_index, event) in Parser::new_ext(markdown, options).enumerate() {
        if event_index % 256 == 0 && cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Err(PreviewError::Cancelled);
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(heading_kind(level)));
            }
            Event::End(TagEnd::Heading(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::Paragraph) => {
                current.get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph));
            }
            Event::End(TagEnd::Paragraph) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::BlockQuote(_)) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(MarkdownBlockKind::Quote));
            }
            Event::End(TagEnd::BlockQuote(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::CodeBlock(kind)) => {
                finalize(&mut blocks, &mut current);
                let mut block = PendingBlock::new(MarkdownBlockKind::CodeBlock);
                block.code_language = code_language(&kind);
                current = Some(block);
            }
            Event::End(TagEnd::CodeBlock) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                finalize(&mut blocks, &mut current);
                let mut item = PendingBlock::new(MarkdownBlockKind::ListItem);
                item.push(
                    MarkdownInlineKind::Text,
                    "",
                    &format!("{}• ", "  ".repeat(list_depth.saturating_sub(1))),
                );
                current = Some(item);
            }
            Event::End(TagEnd::Item) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::Table(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::TableRow) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(MarkdownBlockKind::TableRow));
            }
            Event::End(TagEnd::TableRow) => finalize(&mut blocks, &mut current),
            Event::End(TagEnd::TableCell) => {
                if let Some(block) = current.as_mut() {
                    block.push(MarkdownInlineKind::Text, "", " | ");
                }
            }
            Event::Start(Tag::Strong) => strong = true,
            Event::End(TagEnd::Strong) => strong = false,
            Event::Start(Tag::Emphasis) => emphasis = true,
            Event::End(TagEnd::Emphasis) => emphasis = false,
            Event::Start(Tag::Link { dest_url, .. }) => link_href = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link_href = None,
            Event::Start(Tag::Image { dest_url, .. }) => {
                in_image = true;
                link_href = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                link_href = None;
            }
            Event::TaskListMarker(checked) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::ListItem))
                    .push(
                        MarkdownInlineKind::Text,
                        "",
                        if checked { "[x] " } else { "[ ] " },
                    );
            }
            Event::Text(text) => {
                let block =
                    current.get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph));
                let kind = if link_href.is_some() {
                    MarkdownInlineKind::Link
                } else if strong {
                    MarkdownInlineKind::Strong
                } else if emphasis {
                    MarkdownInlineKind::Emphasis
                } else {
                    MarkdownInlineKind::Text
                };
                if in_image {
                    block.push(MarkdownInlineKind::Text, "", "Image: ");
                }
                block.push(kind, link_href.as_deref().unwrap_or(""), &text);
            }
            Event::Code(text) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Code, "", &text);
            }
            Event::SoftBreak | Event::HardBreak => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Text, "", "\n");
            }
            Event::Rule => {
                finalize(&mut blocks, &mut current);
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::Rule,
                    text: String::new(),
                    spans: Vec::new(),
                    code_language: String::new(),
                    code_spans: Vec::new(),
                });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Text, "", &html);
            }
            _ => {}
        }
    }
    finalize(&mut blocks, &mut current);

    for block in &mut blocks {
        if block.kind == MarkdownBlockKind::CodeBlock {
            block.code_spans = highlight_code(&block.text, &block.code_language, cancellation)?;
        }
    }

    let title = blocks
        .iter()
        .find(|block| {
            matches!(
                block.kind,
                MarkdownBlockKind::Heading1
                    | MarkdownBlockKind::Heading2
                    | MarkdownBlockKind::Heading3
            )
        })
        .map(|block| block.text.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback_title.trim())
        .to_string();
    let title = if title.is_empty() {
        "Markdown document".to_string()
    } else {
        title.chars().take(120).collect()
    };
    let plain_text = blocks
        .iter()
        .filter(|block| block.kind != MarkdownBlockKind::Rule)
        .map(|block| block.text.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(MarkdownDocument {
        title,
        plain_text,
        blocks,
    })
}

#[cfg(feature = "markdown")]
impl PendingBlock {
    fn new(kind: MarkdownBlockKind) -> Self {
        Self {
            kind,
            text: String::new(),
            spans: Vec::new(),
            code_language: String::new(),
        }
    }

    fn push(&mut self, kind: MarkdownInlineKind, href: &str, text: &str) {
        self.text.push_str(text);
        if let Some(last) = self.spans.last_mut()
            && last.kind == kind
            && last.href == href
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(MarkdownInlineSpan {
            kind,
            text: text.to_string(),
            href: href.to_string(),
        });
    }
}

#[cfg(feature = "markdown")]
fn code_language(kind: &CodeBlockKind<'_>) -> String {
    match kind {
        CodeBlockKind::Indented => String::new(),
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|character| matches!(character, '{' | '}' | '.'))
            .to_ascii_lowercase(),
    }
}

#[cfg(feature = "markdown")]
struct MarkdownHighlightResources {
    syntaxes: SyntaxSet,
    light_theme: Theme,
}

#[cfg(feature = "markdown")]
static MARKDOWN_HIGHLIGHT_RESOURCES: LazyLock<MarkdownHighlightResources> = LazyLock::new(|| {
    let mut themes = ThemeSet::load_defaults().themes;
    MarkdownHighlightResources {
        syntaxes: two_face::syntax::extra_newlines(),
        light_theme: themes
            .remove("InspiredGitHub")
            .expect("syntect must bundle the InspiredGitHub theme"),
    }
});

#[cfg(feature = "markdown")]
fn highlight_code(
    source: &str,
    language: &str,
    cancellation: Option<&CancellationToken>,
) -> Result<Vec<MarkdownCodeSpan>, PreviewError> {
    let resources = &*MARKDOWN_HIGHLIGHT_RESOURCES;
    let syntax = resolve_code_syntax(&resources.syntaxes, language);
    let mut highlighter = HighlightLines::new(syntax, &resources.light_theme);
    let mut spans = Vec::<MarkdownCodeSpan>::new();
    for (line_index, line) in source.split_inclusive('\n').enumerate() {
        if line_index % 64 == 0 && cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Err(PreviewError::Cancelled);
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

#[cfg(feature = "markdown")]
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

#[cfg(feature = "markdown")]
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

#[cfg(feature = "markdown")]
fn heading_kind(level: HeadingLevel) -> MarkdownBlockKind {
    match level {
        HeadingLevel::H1 => MarkdownBlockKind::Heading1,
        HeadingLevel::H2 => MarkdownBlockKind::Heading2,
        _ => MarkdownBlockKind::Heading3,
    }
}

pub fn looks_like_markdown(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("```")
        || trimmed.contains('`')
        || trimmed.contains("**")
        || trimmed.contains("__")
        || trimmed.contains("# ")
        || (trimmed.contains('[') && trimmed.contains("]("))
    {
        return true;
    }
    trimmed.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.starts_with("+ ")
            || line.starts_with("> ")
            || line.starts_with("1. ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Instant;

    struct FailingProvider {
        id: &'static str,
        score: u16,
        calls: Arc<AtomicUsize>,
    }

    impl PreviewProvider for FailingProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn label(&self) -> &'static str {
            self.id
        }

        fn probe(&self, _source: &MaterializedSource) -> u16 {
            self.score
        }

        fn prepare(
            &self,
            _source: &MaterializedSource,
            _cancellation: &CancellationToken,
        ) -> Result<PreparedArtifact, PreviewError> {
            self.calls.fetch_add(1, AtomicOrdering::Relaxed);
            Err(PreviewError::Decode(format!("{} failed", self.id)))
        }
    }

    #[test]
    fn markdown_provider_wins_over_plain_text_and_extracts_title() {
        let registry = PreviewRegistry::with_builtins();
        let prepared = registry
            .prepare(
                ArtifactSource::inline_markdown("m1", "Fallback", "# Report\n\n**Done**"),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(prepared.provider_id, MARKDOWN_PROVIDER_ID);
        assert_eq!(prepared.title, "Report");
        let PreparedContent::Markdown(document) = prepared.content else {
            panic!("expected markdown payload");
        };
        assert!(document.plain_text.contains("Done"));
    }

    #[test]
    fn fenced_code_preserves_language_and_generates_theme_aware_tokens() {
        let document = parse_markdown("```javascript\nconst count = 42; // <ready>\n```", "Code");
        let code = document
            .blocks
            .iter()
            .find(|block| block.kind == MarkdownBlockKind::CodeBlock)
            .expect("fenced code block");

        assert_eq!(code.code_language, "javascript");
        assert_eq!(
            code.code_spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            code.text
        );
        assert!(
            code.code_spans
                .iter()
                .map(|span| span.light_color)
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 3
        );
        assert!(
            code.code_spans
                .iter()
                .any(|span| span.light_color != span.dark_color)
        );
    }

    #[test]
    fn typescript_fence_resolves_to_a_highlighted_syntax() {
        let document = parse_markdown(
            "```typescript\nimport { WeCom } from '@picc/jsbridge';\nawait WeCom.register({ appId: 'your-app-id' });\n```",
            "Code",
        );
        let code = document
            .blocks
            .iter()
            .find(|block| block.kind == MarkdownBlockKind::CodeBlock)
            .expect("fenced code block");

        assert_eq!(code.code_language, "typescript");
        assert!(
            code.code_spans
                .iter()
                .map(|span| span.light_color)
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 3,
            "typescript unexpectedly fell back to plain text: {:?}",
            code.code_spans
        );
    }

    #[test]
    fn disabled_provider_does_not_prepare_explicit_markdown() {
        let mut registry = PreviewRegistry::with_builtins();
        assert!(registry.set_enabled(MARKDOWN_PROVIDER_ID, false));
        let result = registry.prepare(
            ArtifactSource::inline_markdown("m1", "Report", "# Report"),
            &CancellationToken::default(),
        );
        assert_eq!(result.unwrap_err(), PreviewError::Unsupported);
    }

    #[test]
    fn cancellation_stops_before_materialization() {
        let registry = PreviewRegistry::with_builtins();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let result = registry.prepare(
            ArtifactSource::inline_markdown("m1", "Report", "# Report"),
            &cancellation,
        );
        assert_eq!(result.unwrap_err(), PreviewError::Cancelled);
    }

    #[test]
    fn cancellation_stops_markdown_parsing_and_highlighting() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();

        assert_eq!(
            parse_markdown_inner("```rust\nfn main() {}\n```", "Code", Some(&cancellation))
                .unwrap_err(),
            PreviewError::Cancelled
        );
    }

    #[test]
    fn selected_provider_failure_does_not_fall_back_to_lower_score() {
        let high_calls = Arc::new(AtomicUsize::new(0));
        let low_calls = Arc::new(AtomicUsize::new(0));
        let mut registry = PreviewRegistry::new();
        registry.register(FailingProvider {
            id: "low",
            score: 10,
            calls: low_calls.clone(),
        });
        registry.register(FailingProvider {
            id: "high",
            score: 20,
            calls: high_calls.clone(),
        });

        let error = registry
            .prepare(
                ArtifactSource {
                    id: "source".into(),
                    title: "Source".into(),
                    mime_type: None,
                    hint: ArtifactHint::Auto,
                    body: ArtifactBody::Text("content".into()),
                },
                &CancellationToken::default(),
            )
            .unwrap_err();

        assert!(matches!(error, PreviewError::Decode(_)));
        assert_eq!(high_calls.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(low_calls.load(AtomicOrdering::Relaxed), 0);
    }

    #[cfg(feature = "image")]
    #[test]
    fn image_magic_selects_image_provider() {
        use image::{DynamicImage, ImageFormat};
        use std::io::Cursor;

        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(2, 3)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let prepared = PreviewRegistry::with_builtins()
            .prepare(
                ArtifactSource {
                    id: "image".into(),
                    title: "sample.png".into(),
                    mime_type: None,
                    hint: ArtifactHint::Auto,
                    body: ArtifactBody::Bytes(bytes.into_inner()),
                },
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(prepared.provider_id, IMAGE_PROVIDER_ID);
        let PreparedContent::Image(image) = prepared.content else {
            panic!("expected image payload");
        };
        assert_eq!((image.width, image.height), (2, 3));
    }

    #[cfg(feature = "markdown")]
    #[test]
    #[ignore = "release-only quantitative benchmark"]
    fn markdown_prepare_release_benchmark() {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
        let section = "## Result\n\n- **Passed**: `cargo test --lib --locked`\n- [Report](https://example.invalid/report)\n\n> Provider preparation is width-independent.\n\n";
        let markdown = format!("# Build report\n\n{}", section.repeat(7_200));
        let source_bytes = markdown.len();
        let registry = PreviewRegistry::with_builtins();
        let mut durations_us = Vec::with_capacity(15);
        for run in 0..15 {
            let source = ArtifactSource::inline_markdown(
                format!("benchmark-{run}"),
                "Benchmark",
                markdown.clone(),
            );
            let started = Instant::now();
            black_box(
                registry
                    .prepare(source, &CancellationToken::default())
                    .unwrap(),
            );
            durations_us.push(started.elapsed().as_micros() as u64);
        }
        durations_us.sort_unstable();
        let median = durations_us[durations_us.len() / 2];
        let p95 = durations_us[(durations_us.len() * 95 / 100).min(durations_us.len() - 1)];
        println!(
            "markdown_prepare_benchmark source_bytes={} median_us={} p95_us={} raw_us={:?}",
            source_bytes, median, p95, durations_us,
        );
    }
}
