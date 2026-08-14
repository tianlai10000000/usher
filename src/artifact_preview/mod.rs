//! Application adapter for the framework-neutral `usher-preview` registry.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use slint::{ComponentHandle, Global, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use usher_preview::{
    CancellationToken, IMAGE_PROVIDER_ID, MARKDOWN_PROVIDER_ID, MarkdownBlock as CoreMarkdownBlock,
    MarkdownBlockKind as CoreBlockKind, MarkdownInlineKind, PLAIN_TEXT_PROVIDER_ID,
    PreparedArtifact, PreparedContent, PreviewRegistry,
};

pub use usher_preview::{ArtifactBody, ArtifactHint, ArtifactSource};

use crate::app_runtime::shared_runtime;
use crate::settings::PreviewSettings;
use crate::{
    AppState, ArtifactPreviewBlock, ArtifactPreviewBlockKind, ArtifactPreviewInlineKind,
    ArtifactPreviewInlineSpan, ArtifactPreviewKind, ArtifactPreviewWindow, MainWindow,
};

#[cfg(feature = "test-fixtures")]
mod fixtures;
pub(crate) mod performance;
mod virtual_layout;

use virtual_layout::MarkdownVirtualLayoutBridge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationMode {
    Docked,
    Detached,
}

impl PresentationMode {
    fn from_index(index: i32) -> Self {
        if index == 1 {
            Self::Detached
        } else {
            Self::Docked
        }
    }

    fn setting_value(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::Detached => "detached",
        }
    }
}

#[derive(Clone)]
struct UiPayload {
    title: slint::SharedString,
    kind: ArtifactPreviewKind,
    blocks: ModelRc<ArtifactPreviewBlock>,
    plain_text: slint::SharedString,
    image: slint::Image,
    error: slint::SharedString,
    can_copy: bool,
    can_open_external: bool,
    copy_content: slint::SharedString,
    provider_id: Option<&'static str>,
    source_path: Option<PathBuf>,
    approximate_bytes: usize,
}

impl UiPayload {
    fn error(title: String, message: String) -> Self {
        Self {
            title: title.into(),
            kind: ArtifactPreviewKind::Error,
            blocks: ModelRc::default(),
            plain_text: "".into(),
            image: slint::Image::default(),
            error: message.into(),
            can_copy: false,
            can_open_external: false,
            copy_content: "".into(),
            provider_id: None,
            source_path: None,
            approximate_bytes: 0,
        }
    }
}

struct PreviewUiRuntime {
    mode: PresentationMode,
    current: Option<UiPayload>,
    loading_title: Option<slint::SharedString>,
    detached: Option<ArtifactPreviewWindow>,
}

impl Default for PreviewUiRuntime {
    fn default() -> Self {
        Self {
            mode: PresentationMode::Docked,
            current: None,
            loading_title: None,
            detached: None,
        }
    }
}

thread_local! {
    static PREVIEW_UI: RefCell<PreviewUiRuntime> = RefCell::new(PreviewUiRuntime::default());
}

static HOST_VISUAL_GENERATION: AtomicU64 = AtomicU64::new(0);

struct PreviewService {
    registry: RwLock<PreviewRegistry>,
    generation: AtomicU64,
    cancellation: Mutex<Option<CancellationToken>>,
}

impl PreviewService {
    fn new(settings: &PreviewSettings) -> Self {
        let mut registry = PreviewRegistry::with_builtins();
        registry.set_enabled(MARKDOWN_PROVIDER_ID, settings.providers.markdown);
        registry.set_enabled(IMAGE_PROVIDER_ID, settings.providers.image);
        registry.set_enabled(PLAIN_TEXT_PROVIDER_ID, settings.providers.plain_text);
        Self {
            registry: RwLock::new(registry),
            generation: AtomicU64::new(0),
            cancellation: Mutex::new(None),
        }
    }

    fn begin_request(&self) -> (u64, CancellationToken) {
        if let Some(previous) = self
            .cancellation
            .lock()
            .expect("preview cancellation mutex poisoned")
            .take()
        {
            previous.cancel();
        }
        let token = CancellationToken::default();
        *self
            .cancellation
            .lock()
            .expect("preview cancellation mutex poisoned") = Some(token.clone());
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        (generation, token)
    }

    fn cancel_current(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(token) = self
            .cancellation
            .lock()
            .expect("preview cancellation mutex poisoned")
            .take()
        {
            token.cancel();
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }
}

#[derive(Clone)]
pub struct ArtifactPreviewHandle {
    service: Arc<PreviewService>,
    ui: slint::Weak<MainWindow>,
}

impl ArtifactPreviewHandle {
    pub fn open_source(&self, source: ArtifactSource) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let title = source.title.clone();
        let (generation, cancellation) = self.service.begin_request();
        show_loading(&ui, title.clone());

        let registry = self
            .service
            .registry
            .read()
            .expect("preview registry poisoned")
            .clone();
        let service = self.service.clone();
        let ui_weak = self.ui.clone();
        shared_runtime().spawn(async move {
            let prepared = tokio::task::spawn_blocking(move || {
                let prepared = registry.prepare(source, &cancellation)?;
                adapt_prepared_worker(prepared, &cancellation)
            })
            .await;

            let result = match prepared {
                Ok(result) => result,
                Err(error) => Err(usher_preview::PreviewError::Decode(format!(
                    "preview worker failed: {error}"
                ))),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if !service.is_current(generation) {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                let payload = match result {
                    Ok(prepared) => prepared.into_ui_payload(),
                    Err(error) => UiPayload::error(title, user_safe_error(&error)),
                };
                show_payload(&ui, payload);
            });
        });
    }

    pub fn open_file(&self, path: impl Into<PathBuf>, mime_type: Option<String>) {
        self.open_source(ArtifactSource::file(path, mime_type));
    }
}

pub(crate) fn setup(ui: &MainWindow, settings: &PreviewSettings) -> ArtifactPreviewHandle {
    let performance_optimized =
        std::env::var("USHER_PREVIEW_PERF_VARIANT").as_deref() != Ok("baseline");
    ui.set_artifact_preview_performance_optimized(performance_optimized);
    log::info!(
        "[ArtifactPreview::Performance] variant={}",
        if performance_optimized {
            "optimized"
        } else {
            "baseline"
        }
    );
    let docked_virtual_layout =
        Rc::new(RefCell::new(MarkdownVirtualLayoutBridge::from_environment()));
    let layout_bridge = docked_virtual_layout.clone();
    ui.on_artifact_preview_layout_markdown_window(
        move |revision, blocks, scroll_offset, visible_height, content_width, font_family| {
            let mut layout_bridge = layout_bridge.borrow_mut();
            let window = layout_bridge.layout(
                revision,
                &blocks,
                scroll_offset,
                visible_height,
                content_width,
                font_family.as_str(),
            );
            if let Some((projected, mounted_rows)) = layout_bridge.take_scroll_activity() {
                performance::mark_scroll("docked-divider", projected, mounted_rows);
            }
            window
        },
    );
    let service = Arc::new(PreviewService::new(settings));
    if memory_benchmark_fixture_config().is_some() {
        service
            .registry
            .write()
            .expect("preview registry poisoned")
            .set_enabled(MARKDOWN_PROVIDER_ID, true);
    }
    let handle = ArtifactPreviewHandle {
        service: service.clone(),
        ui: ui.as_weak(),
    };

    PREVIEW_UI.with(|runtime| {
        runtime.borrow_mut().mode = if settings.presentation_mode == "detached" {
            PresentationMode::Detached
        } else {
            PresentationMode::Docked
        };
    });

    let open_handle = handle.clone();
    ui.on_chat_open_artifact(move |id, title, markdown| {
        open_handle.open_source(ArtifactSource::inline_markdown(id, title, markdown));
    });

    let service_for_close = service.clone();
    let ui_weak = ui.as_weak();
    ui.on_artifact_preview_dismiss(move || {
        service_for_close.cancel_current();
        if let Some(ui) = ui_weak.upgrade() {
            clear_hosts(&ui);
        }
    });

    ui.on_artifact_preview_copy(move |text| copy_to_clipboard(text.as_str()));

    ui.on_artifact_preview_open_external(move || {
        let path = PREVIEW_UI.with(|runtime| {
            runtime
                .borrow()
                .current
                .as_ref()
                .and_then(|payload| payload.source_path.clone())
        });
        if let Some(path) = path
            && let Err(error) = crate::platform::open_file_with_system_application(&path)
        {
            log::error!("[ArtifactPreview] {error}");
        }
    });

    ui.on_artifact_preview_open_link(move |url| {
        let url = url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            log::warn!("[ArtifactPreview] Refused non-HTTP link");
            return;
        }
        if let Err(error) = webbrowser::open(url) {
            log::error!("[ArtifactPreview] Failed to open link: {error}");
        }
    });

    let ui_weak = ui.as_weak();
    AppState::get(ui).on_set_artifact_preview_presentation_mode(move |index| {
        let mode = PresentationMode::from_index(index);
        if let Some(ui) = ui_weak.upgrade() {
            AppState::get(&ui).set_artifact_preview_presentation_mode(index.clamp(0, 1));
            switch_mode(&ui, mode);
        }
        let setting = mode.setting_value().to_string();
        shared_runtime().spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::settings::save_preview_presentation_mode(&setting)
            })
            .await;
            if let Ok(Err(error)) = result {
                log::error!("[ArtifactPreview] Failed to persist presentation mode: {error}");
            }
        });
    });

    let provider_service = service.clone();
    let ui_weak = ui.as_weak();
    AppState::get(ui).on_set_artifact_preview_provider_enabled(move |provider_id, enabled| {
        let provider_id = provider_id.to_string();
        let changed = provider_service
            .registry
            .write()
            .expect("preview registry poisoned")
            .set_enabled(&provider_id, enabled);
        if !changed {
            log::warn!("[ArtifactPreview] Unknown provider setting: {provider_id}");
            return;
        }
        if let Some(ui) = ui_weak.upgrade() {
            let state = AppState::get(&ui);
            match provider_id.as_str() {
                MARKDOWN_PROVIDER_ID => state.set_artifact_preview_markdown_enabled(enabled),
                IMAGE_PROVIDER_ID => state.set_artifact_preview_image_enabled(enabled),
                PLAIN_TEXT_PROVIDER_ID => state.set_artifact_preview_plain_text_enabled(enabled),
                _ => {}
            }
            let active_provider = PREVIEW_UI.with(|runtime| {
                runtime
                    .borrow()
                    .current
                    .as_ref()
                    .and_then(|payload| payload.provider_id)
            });
            if !enabled && active_provider == Some(provider_id.as_str()) {
                provider_service.cancel_current();
                clear_hosts(&ui);
            }
        }
        shared_runtime().spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::settings::save_preview_provider_enabled(&provider_id, enabled)
            })
            .await;
            if let Ok(Err(error)) = result {
                log::error!("[ArtifactPreview] Failed to persist provider setting: {error}");
            }
        });
    });

    handle
}

#[cfg(feature = "test-fixtures")]
pub(crate) fn setup_fixtures(ui: &MainWindow, handle: ArtifactPreviewHandle) {
    fixtures::setup(ui, handle);
}

struct UiPreparedPayload {
    title: String,
    content: UiPreparedContent,
    can_copy: bool,
    can_open_external: bool,
    copy_content: String,
    provider_id: Option<&'static str>,
    source_path: Option<PathBuf>,
    approximate_bytes: usize,
}

enum UiPreparedContent {
    Markdown(Vec<UiPreparedMarkdownBlock>),
    PlainText(String),
    Image(usher_preview::DecodedImage),
}

struct UiPreparedMarkdownBlock {
    kind: ArtifactPreviewBlockKind,
    styled: slint::StyledText,
    plain_text: String,
    layout_spans: Vec<ArtifactPreviewInlineSpan>,
}

impl UiPreparedPayload {
    fn into_ui_payload(self) -> UiPayload {
        let (kind, blocks, plain_text, image) = match self.content {
            UiPreparedContent::Markdown(blocks) => {
                let blocks = blocks
                    .into_iter()
                    .map(UiPreparedMarkdownBlock::into_ui_block)
                    .collect::<Vec<_>>();
                (
                    ArtifactPreviewKind::Markdown,
                    ModelRc::new(Rc::new(VecModel::from(blocks))),
                    "".into(),
                    slint::Image::default(),
                )
            }
            UiPreparedContent::PlainText(text) => (
                ArtifactPreviewKind::PlainText,
                ModelRc::default(),
                text.into(),
                slint::Image::default(),
            ),
            UiPreparedContent::Image(image) => {
                let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &image.rgba,
                    image.width,
                    image.height,
                );
                (
                    ArtifactPreviewKind::Image,
                    ModelRc::default(),
                    "".into(),
                    slint::Image::from_rgba8(pixels),
                )
            }
        };
        UiPayload {
            title: self.title.into(),
            kind,
            blocks,
            plain_text,
            image,
            error: "".into(),
            can_copy: self.can_copy,
            can_open_external: self.can_open_external,
            copy_content: self.copy_content.into(),
            provider_id: self.provider_id,
            source_path: self.source_path,
            approximate_bytes: self.approximate_bytes,
        }
    }
}

fn adapt_prepared_worker(
    prepared: PreparedArtifact,
    cancellation: &CancellationToken,
) -> Result<UiPreparedPayload, usher_preview::PreviewError> {
    let source_path = prepared.source_path.clone();
    let provider_id = Some(prepared.provider_id);
    let can_copy = prepared.capabilities.copy;
    let can_open_external = prepared.capabilities.open_external;
    let approximate_bytes = prepared.approximate_bytes;
    let payload = match prepared.content {
        PreparedContent::Markdown(document) => {
            let blocks = document
                .blocks
                .iter()
                .enumerate()
                .map(|(index, block)| {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(usher_preview::PreviewError::Cancelled);
                    }
                    Ok(adapt_markdown_block(block))
                })
                .collect::<Result<Vec<_>, usher_preview::PreviewError>>()?;
            UiPreparedPayload {
                title: prepared.title,
                content: UiPreparedContent::Markdown(blocks),
                can_copy,
                can_open_external,
                copy_content: document.plain_text,
                provider_id,
                source_path,
                approximate_bytes,
            }
        }
        PreparedContent::PlainText(text) => UiPreparedPayload {
            title: prepared.title,
            content: UiPreparedContent::PlainText(text.clone()),
            can_copy,
            can_open_external,
            copy_content: text,
            provider_id,
            source_path,
            approximate_bytes,
        },
        PreparedContent::Image(image) => UiPreparedPayload {
            title: prepared.title,
            content: UiPreparedContent::Image(image),
            can_copy,
            can_open_external,
            copy_content: String::new(),
            provider_id,
            source_path,
            approximate_bytes,
        },
    };
    if cancellation.is_cancelled() {
        Err(usher_preview::PreviewError::Cancelled)
    } else {
        Ok(payload)
    }
}

impl UiPreparedMarkdownBlock {
    fn into_ui_block(self) -> ArtifactPreviewBlock {
        ArtifactPreviewBlock {
            kind: self.kind,
            styled: self.styled,
            plain_text: self.plain_text.into(),
            layout_spans: ModelRc::new(Rc::new(VecModel::from(self.layout_spans))),
        }
    }
}

fn adapt_markdown_block(block: &CoreMarkdownBlock) -> UiPreparedMarkdownBlock {
    let mut byte_offset = 0usize;
    let layout_spans = block
        .spans
        .iter()
        .filter_map(|span| {
            let start = byte_offset;
            byte_offset = byte_offset.saturating_add(span.text.len());
            let end = byte_offset;
            let start = i32::try_from(start).ok()?;
            let end = i32::try_from(end).ok()?;
            Some(ArtifactPreviewInlineSpan {
                kind: match span.kind {
                    MarkdownInlineKind::Text => ArtifactPreviewInlineKind::Text,
                    MarkdownInlineKind::Strong => ArtifactPreviewInlineKind::Strong,
                    MarkdownInlineKind::Emphasis => ArtifactPreviewInlineKind::Emphasis,
                    MarkdownInlineKind::Code => ArtifactPreviewInlineKind::Code,
                    MarkdownInlineKind::Link => ArtifactPreviewInlineKind::Link,
                },
                start,
                end,
            })
        })
        .collect::<Vec<_>>();
    UiPreparedMarkdownBlock {
        kind: match block.kind {
            CoreBlockKind::Heading1 => ArtifactPreviewBlockKind::Heading1,
            CoreBlockKind::Heading2 => ArtifactPreviewBlockKind::Heading2,
            CoreBlockKind::Heading3 => ArtifactPreviewBlockKind::Heading3,
            CoreBlockKind::Paragraph => ArtifactPreviewBlockKind::Paragraph,
            CoreBlockKind::ListItem => ArtifactPreviewBlockKind::ListItem,
            CoreBlockKind::Quote => ArtifactPreviewBlockKind::Quote,
            CoreBlockKind::CodeBlock => ArtifactPreviewBlockKind::CodeBlock,
            CoreBlockKind::Rule => ArtifactPreviewBlockKind::Rule,
            CoreBlockKind::TableRow => ArtifactPreviewBlockKind::TableRow,
        },
        styled: styled_text_for(block),
        plain_text: block.text.clone(),
        layout_spans,
    }
}

fn styled_text_for(block: &CoreMarkdownBlock) -> slint::StyledText {
    if matches!(
        block.kind,
        CoreBlockKind::CodeBlock | CoreBlockKind::TableRow
    ) {
        return slint::StyledText::from_plain_text(&block.text);
    }
    let mut markdown = String::new();
    let heading = matches!(
        block.kind,
        CoreBlockKind::Heading1 | CoreBlockKind::Heading2 | CoreBlockKind::Heading3
    );
    if heading {
        markdown.push_str("**");
    }
    for span in &block.spans {
        let text = escape_inline_markdown(&span.text);
        match span.kind {
            MarkdownInlineKind::Text => markdown.push_str(&text),
            MarkdownInlineKind::Strong => markdown.push_str(&format!("**{text}**")),
            MarkdownInlineKind::Emphasis => markdown.push_str(&format!("*{text}*")),
            MarkdownInlineKind::Code => markdown.push_str(&format!("`{text}`")),
            MarkdownInlineKind::Link => {
                markdown.push_str(&format!("[{text}]({})", span.href.replace(')', "\\)")))
            }
        }
    }
    if heading {
        markdown.push_str("**");
    }
    slint::StyledText::from_markdown(&markdown)
        .unwrap_or_else(|_| slint::StyledText::from_plain_text(&block.text))
}

fn escape_inline_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '*' | '_' | '[' | ']' | '`') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn show_loading(ui: &MainWindow, title: String) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        runtime.current = None;
        runtime.loading_title = Some(title.into());
        present_runtime(ui, &mut runtime);
    });
}

fn show_payload(ui: &MainWindow, payload: UiPayload) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        runtime.loading_title = None;
        runtime.current = Some(payload);
        present_runtime(ui, &mut runtime);
    });
}

fn switch_mode(ui: &MainWindow, mode: PresentationMode) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        if runtime.mode == mode {
            return;
        }
        runtime.mode = mode;
        present_runtime(ui, &mut runtime);
    });
}

fn present_runtime(ui: &MainWindow, runtime: &mut PreviewUiRuntime) {
    let has_content = runtime.current.is_some() || runtime.loading_title.is_some();
    if !has_content {
        if runtime.mode == PresentationMode::Docked {
            begin_main_host_close(ui);
        } else {
            clear_main_host_immediately(ui);
        }
        if let Some(window) = runtime.detached.take() {
            let _ = window.hide();
        }
        return;
    }

    match runtime.mode {
        PresentationMode::Docked => {
            if let Some(payload) = runtime.current.as_ref() {
                apply_payload_to_main(ui, payload);
            } else if let Some(title) = runtime.loading_title.as_ref() {
                apply_loading_to_main(ui, title);
            }
            if let Some(window) = runtime.detached.take() {
                let _ = window.hide();
            }
        }
        PresentationMode::Detached => {
            let Some(window) = ensure_detached_window(ui, runtime) else {
                return;
            };
            if let Some(payload) = runtime.current.as_ref() {
                apply_payload_to_window(&window, payload);
            } else if let Some(title) = runtime.loading_title.as_ref() {
                apply_loading_to_window(&window, title);
            }
            center_detached_window(ui, &window);
            if let Err(error) = window.show() {
                log::error!("[ArtifactPreview] Failed to show detached window: {error}");
            } else {
                install_detached_live_resize_bridge(&window);
            }
            clear_main_host_immediately(ui);
        }
    }
}

fn install_detached_live_resize_bridge(window: &ArtifactPreviewWindow) {
    // winit creates the NSWindow asynchronously after Slint's show() request.
    // Try immediately, then retry on bounded event-loop turns. The platform
    // adapter is idempotent once it observes the current native window.
    crate::platform::install_artifact_preview_live_resize(window);
    for delay in [30, 120] {
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(delay), move || {
            if let Some(window) = weak.upgrade() {
                crate::platform::install_artifact_preview_live_resize(&window);
            }
        });
    }
}

fn ensure_detached_window(
    ui: &MainWindow,
    runtime: &mut PreviewUiRuntime,
) -> Option<ArtifactPreviewWindow> {
    if let Some(window) = runtime.detached.as_ref() {
        return Some(window.clone_strong());
    }
    let window = match ArtifactPreviewWindow::new() {
        Ok(window) => window,
        Err(error) => {
            log::error!("[ArtifactPreview] Failed to create detached window: {error}");
            return None;
        }
    };
    window.set_application_font_family(ui.get_application_font_family());
    window.set_performance_optimized(ui.get_artifact_preview_performance_optimized());
    let detached_virtual_layout =
        Rc::new(RefCell::new(MarkdownVirtualLayoutBridge::from_environment()));
    window.on_layout_markdown_window(
        move |revision, blocks, scroll_offset, visible_height, content_width, font_family| {
            let mut layout_bridge = detached_virtual_layout.borrow_mut();
            let window = layout_bridge.layout(
                revision,
                &blocks,
                scroll_offset,
                visible_height,
                content_width,
                font_family.as_str(),
            );
            if let Some((projected, mounted_rows)) = layout_bridge.take_scroll_activity() {
                performance::mark_scroll("detached-window", projected, mounted_rows);
            }
            window
        },
    );
    performance::install_detached_render_trace(&window);
    let ui_weak = ui.as_weak();
    window.on_dismiss_requested(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_artifact_preview_dismiss();
        }
    });
    let ui_weak = ui.as_weak();
    window.on_copy_requested(move |text| {
        ui_weak
            .upgrade()
            .is_some_and(|ui| ui.invoke_artifact_preview_copy(text))
    });
    let ui_weak = ui.as_weak();
    window.on_open_external_requested(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_artifact_preview_open_external();
        }
    });
    let ui_weak = ui.as_weak();
    window.on_open_link(move |url| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_artifact_preview_open_link(url);
        }
    });
    let ui_weak = ui.as_weak();
    window.window().on_close_requested(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_artifact_preview_dismiss();
        }
        slint::CloseRequestResponse::HideWindow
    });
    runtime.detached = Some(window);
    runtime.detached.as_ref().map(ComponentHandle::clone_strong)
}

fn center_detached_window(ui: &MainWindow, window: &ArtifactPreviewWindow) {
    let parent_position = ui.window().position();
    let parent_size = ui.window().size();
    let child_size = window.window().size();
    if child_size.width == 0 || child_size.height == 0 {
        return;
    }
    window.window().set_position(slint::PhysicalPosition::new(
        parent_position.x + (parent_size.width as i32 - child_size.width as i32) / 2,
        parent_position.y + (parent_size.height as i32 - child_size.height as i32) / 2,
    ));
}

fn apply_payload_to_main(ui: &MainWindow, payload: &UiPayload) {
    let revision = next_layout_revision();
    ui.set_artifact_preview_title(payload.title.clone());
    ui.set_artifact_preview_kind(payload.kind);
    ui.set_artifact_preview_blocks(payload.blocks.clone());
    ui.set_artifact_preview_markdown_layout_revision(revision);
    ui.set_artifact_preview_plain_text(payload.plain_text.clone());
    ui.set_artifact_preview_image(payload.image.clone());
    ui.set_artifact_preview_error(payload.error.clone());
    ui.set_artifact_preview_can_copy(payload.can_copy);
    ui.set_artifact_preview_can_open_external(payload.can_open_external);
    ui.set_artifact_preview_copy_content(payload.copy_content.clone());
    ui.set_artifact_preview_loading(false);
    ui.set_artifact_preview_visible(true);
}

fn apply_loading_to_main(ui: &MainWindow, title: &slint::SharedString) {
    HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel);
    clear_main_payload(ui);
    ui.set_artifact_preview_title(title.clone());
    ui.set_artifact_preview_loading(true);
    ui.set_artifact_preview_visible(true);
}

fn apply_payload_to_window(window: &ArtifactPreviewWindow, payload: &UiPayload) {
    window.set_document_title(payload.title.clone());
    window.set_preview_kind(payload.kind);
    window.set_markdown_blocks(payload.blocks.clone());
    window.set_markdown_layout_revision(next_layout_revision());
    window.set_plain_text(payload.plain_text.clone());
    window.set_preview_image(payload.image.clone());
    window.set_error_text(payload.error.clone());
    window.set_can_copy(payload.can_copy);
    window.set_can_open_external(payload.can_open_external);
    window.set_copy_content(payload.copy_content.clone());
    window.set_loading(false);
}

fn next_layout_revision() -> i32 {
    (HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel) as i32).wrapping_add(1)
}

fn apply_loading_to_window(window: &ArtifactPreviewWindow, title: &slint::SharedString) {
    clear_window_payload(window);
    window.set_document_title(title.clone());
    window.set_loading(true);
}

fn clear_hosts(ui: &MainWindow) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        let animate_docked_close = runtime.mode == PresentationMode::Docked;
        runtime.current = None;
        runtime.loading_title = None;
        if animate_docked_close {
            begin_main_host_close(ui);
        } else {
            clear_main_host_immediately(ui);
        }
        if let Some(window) = runtime.detached.take() {
            clear_window_payload(&window);
            let _ = window.hide();
        }
    });
}

fn begin_main_host_close(ui: &MainWindow) {
    let visual_generation = HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    ui.set_artifact_preview_visible(false);
    ui.set_artifact_preview_loading(false);

    // Retain only the presentation ModelRc for the bounded exit transition.
    // The feature-owned prepared session has already been dropped above. This
    // lets the complete pane retract instead of clearing hundreds of delegates
    // in the same frame as the animation starts.
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(280), move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_artifact_preview_visible()
            && HOST_VISUAL_GENERATION.load(Ordering::Acquire) == visual_generation
        {
            clear_main_payload(&ui);
        }
    });
}

fn clear_main_host_immediately(ui: &MainWindow) {
    HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel);
    ui.set_artifact_preview_visible(false);
    ui.set_artifact_preview_loading(false);
    clear_main_payload(ui);
}

fn clear_main_payload(ui: &MainWindow) {
    ui.set_artifact_preview_kind(ArtifactPreviewKind::None);
    ui.set_artifact_preview_blocks(ModelRc::default());
    ui.set_artifact_preview_plain_text("".into());
    ui.set_artifact_preview_image(slint::Image::default());
    ui.set_artifact_preview_error("".into());
    ui.set_artifact_preview_can_copy(false);
    ui.set_artifact_preview_can_open_external(false);
    ui.set_artifact_preview_copy_content("".into());
}

fn clear_window_payload(window: &ArtifactPreviewWindow) {
    window.set_preview_kind(ArtifactPreviewKind::None);
    window.set_markdown_blocks(ModelRc::default());
    window.set_plain_text("".into());
    window.set_preview_image(slint::Image::default());
    window.set_error_text("".into());
    window.set_can_copy(false);
    window.set_can_open_external(false);
    window.set_copy_content("".into());
    window.set_loading(false);
}

fn copy_to_clipboard(text: &str) -> bool {
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => true,
        Err(error) => {
            log::error!("[ArtifactPreview] Clipboard write failed: {error}");
            false
        }
    }
}

fn user_safe_error(error: &usher_preview::PreviewError) -> String {
    match error {
        usher_preview::PreviewError::Unsupported => {
            "This file type is unsupported or its preview provider is disabled.".into()
        }
        usher_preview::PreviewError::SourceTooLarge { .. } => {
            "This file is too large to preview safely.".into()
        }
        usher_preview::PreviewError::Cancelled => "Preview was cancelled.".into(),
        usher_preview::PreviewError::InvalidUtf8 => "This file is not valid UTF-8 text.".into(),
        usher_preview::PreviewError::Io(_) => "The file could not be read.".into(),
        usher_preview::PreviewError::Decode(_) => "The file could not be decoded.".into(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewDiagnosticState {
    pub host: &'static str,
    pub kind: &'static str,
    pub host_count: usize,
    pub block_count: usize,
    pub approximate_bytes: usize,
}

pub(crate) fn diagnostic_state(ui: &MainWindow) -> PreviewDiagnosticState {
    PREVIEW_UI.with(|runtime| {
        let runtime = runtime.borrow();
        let docked = ui.get_artifact_preview_visible();
        let detached = runtime.detached.is_some();
        let host = match (docked, detached) {
            (true, false) => "docked",
            (false, true) => "detached",
            (false, false) => "none",
            (true, true) => "invalid-both",
        };
        let (kind, block_count, approximate_bytes) =
            runtime.current.as_ref().map_or(("none", 0, 0), |payload| {
                let kind = match payload.kind {
                    ArtifactPreviewKind::Markdown => "markdown",
                    ArtifactPreviewKind::Image => "image",
                    ArtifactPreviewKind::PlainText => "plain-text",
                    ArtifactPreviewKind::Error => "error",
                    ArtifactPreviewKind::None => "none",
                };
                (kind, payload.blocks.row_count(), payload.approximate_bytes)
            });
        PreviewDiagnosticState {
            host,
            kind,
            host_count: usize::from(docked) + usize::from(detached),
            block_count,
            approximate_bytes,
        }
    })
}

static MEMORY_FIXTURE_MOUNTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn maybe_mount_memory_benchmark_fixture(ui: &MainWindow, route: crate::Route) {
    if !crate::memory_diagnostics::diagnostics_enabled() || route != crate::Route::Chat {
        return;
    }
    let Some((mode, close_after_prepare)) = memory_benchmark_fixture_config() else {
        return;
    };
    if MEMORY_FIXTURE_MOUNTED.swap(true, Ordering::AcqRel) {
        return;
    }

    switch_mode(ui, mode);
    let ui_weak = ui.as_weak();
    shared_runtime().spawn(async move {
        let markdown = tokio::task::spawn_blocking(memory_benchmark_markdown_fixture)
            .await
            .unwrap_or_default();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ui.invoke_chat_open_artifact(
                "memory:artifact-preview".into(),
                "Artifact preview benchmark".into(),
                markdown.into(),
            );
            let ui_weak = ui.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_secs(1), move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if close_after_prepare {
                    ui.invoke_artifact_preview_dismiss();
                }
                crate::memory_diagnostics::log_ui_models(&ui, ui.get_current_route());
                crate::memory_diagnostics::log_checkpoint(if close_after_prepare {
                    "artifact_preview.fixture.closed"
                } else {
                    "artifact_preview.fixture.settled"
                });
                if !close_after_prepare {
                    log::info!(
                        "[ArtifactPreview::PerformanceFixture] settled host={} blocks={} divider_x={:.1}",
                        if mode == PresentationMode::Detached {
                            "detached"
                        } else {
                            "docked"
                        },
                        diagnostic_state(&ui).block_count,
                        ui.get_artifact_preview_divider_x(),
                    );
                    schedule_transition_benchmark_fixture(&ui);
                }
            });
        });
    });
}

fn schedule_transition_benchmark_fixture(ui: &MainWindow) {
    if std::env::var("USHER_PREVIEW_TRANSITION_BENCHMARK").as_deref() != Ok("1") {
        return;
    }
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(700), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_artifact_preview_visible(false);
            log::info!("[ArtifactPreview::TransitionFixture] close-started");
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(1_800), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_artifact_preview_visible(true);
            log::info!("[ArtifactPreview::TransitionFixture] open-started");
        }
    });
    slint::Timer::single_shot(std::time::Duration::from_millis(2_400), || {
        log::info!("[ArtifactPreview::TransitionFixture] complete");
    });
}

fn memory_benchmark_fixture_config() -> Option<(PresentationMode, bool)> {
    if !crate::memory_diagnostics::diagnostics_enabled() {
        return None;
    }
    match std::env::var("USHER_MEMORY_SCENARIO").ok()?.as_str() {
        "chat-preview-docked" => Some((PresentationMode::Docked, false)),
        "chat-preview-detached" => Some((PresentationMode::Detached, false)),
        "chat-preview-after-close" => Some((PresentationMode::Docked, true)),
        _ => None,
    }
}

fn memory_benchmark_markdown_fixture() -> String {
    const TARGET_BYTES: usize = 256 * 1024;
    const SECTION_START: &str = "## Result\n\n- **Passed**: `cargo test --lib --locked`\n- [Report](https://example.invalid/report)\n\n> Width-independent projection keeps Chat resize cheap.\n\n```rust\n";
    const CODE_LINE: &str = "let preview = registry.prepare(source, &cancellation)?;\n";
    const SECTION_END: &str =
        "```\n\n| Metric | Value |\n| --- | ---: |\n| model commits | 2 |\n| projections | 1 |\n\n";
    let mut markdown = String::with_capacity(TARGET_BYTES);
    markdown.push_str("# Artifact preview benchmark\n\n");
    let section_bytes = SECTION_START.len() + CODE_LINE.len() * 96 + SECTION_END.len();
    while markdown.len() + section_bytes <= TARGET_BYTES {
        markdown.push_str(SECTION_START);
        for _ in 0..96 {
            markdown.push_str(CODE_LINE);
        }
        markdown.push_str(SECTION_END);
    }
    markdown.push_str("## Appendix\n\n");
    markdown.push_str(&"x".repeat(TARGET_BYTES - markdown.len()));
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_mode_maps_unknown_values_to_docked() {
        assert_eq!(PresentationMode::from_index(-1), PresentationMode::Docked);
        assert_eq!(PresentationMode::from_index(0), PresentationMode::Docked);
        assert_eq!(PresentationMode::from_index(1), PresentationMode::Detached);
        assert_eq!(PresentationMode::from_index(2), PresentationMode::Docked);
    }

    #[test]
    fn beginning_a_new_request_invalidates_the_previous_generation() {
        let service = PreviewService::new(&PreviewSettings::default());
        let (first_generation, first_token) = service.begin_request();
        let (second_generation, _) = service.begin_request();

        assert!(first_token.is_cancelled());
        assert!(!service.is_current(first_generation));
        assert!(service.is_current(second_generation));
    }

    #[test]
    fn inline_markdown_escaping_preserves_literal_punctuation() {
        assert_eq!(escape_inline_markdown("a * b [c]"), "a \\* b \\[c\\]");
    }

    #[test]
    fn memory_benchmark_fixture_has_a_fixed_size() {
        assert_eq!(memory_benchmark_markdown_fixture().len(), 256 * 1024);
    }
}
