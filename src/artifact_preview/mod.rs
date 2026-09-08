//! Application adapter for the framework-neutral `usher-preview` registry.

use std::cell::RefCell;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use slint::{ComponentHandle, Global, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use usher_preview::{
    CancellationToken, IMAGE_PROVIDER_ID, MARKDOWN_PROVIDER_ID, MAX_SOURCE_BYTES,
    MarkdownBlock as CoreMarkdownBlock, MarkdownBlockKind as CoreBlockKind, MarkdownInlineKind,
    PLAIN_TEXT_PROVIDER_ID, PreparedArtifact, PreparedContent, PreviewPreparationProgress,
    PreviewRegistry,
};

pub use usher_preview::{ArtifactBody, ArtifactHint, ArtifactSource};

use crate::app_runtime::shared_runtime;
use crate::settings::PreviewSettings;
use crate::{
    AppState, ArtifactPreviewBlock, ArtifactPreviewBlockKind, ArtifactPreviewCodeLine,
    ArtifactPreviewCodeToken, ArtifactPreviewInlineKind, ArtifactPreviewInlineSpan,
    ArtifactPreviewKind, ArtifactPreviewWindow, MainWindow,
};

mod benchmark;
#[cfg(feature = "test-fixtures")]
mod fixtures;
mod measurement;
pub(crate) mod performance;
mod projection_model;
mod virtual_layout;

use virtual_layout::MarkdownVirtualLayoutBridge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationMode {
    Docked,
    Detached,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArtifactPreviewContext {
    #[default]
    Application,
    PrReviewSidePanel,
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
    error_key: Option<&'static str>,
    can_copy: bool,
    can_open_external: bool,
    copy_content: slint::SharedString,
    provider_id: Option<&'static str>,
    source_path: Option<PathBuf>,
    approximate_bytes: usize,
}

impl UiPayload {
    fn error(title: String, key: &'static str) -> Self {
        Self {
            title: title.into(),
            kind: ArtifactPreviewKind::Error,
            blocks: ModelRc::default(),
            plain_text: "".into(),
            image: slint::Image::default(),
            error: crate::i18n::tr(key).into_owned().into(),
            error_key: Some(key),
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
    context: ArtifactPreviewContext,
    current: Option<UiPayload>,
    loading_title: Option<slint::SharedString>,
    loading_message: slint::SharedString,
    loading_copy: crate::i18n::LocalizedText,
    detached: Option<ArtifactPreviewWindow>,
    docked_virtual_layout: Rc<RefCell<MarkdownVirtualLayoutBridge>>,
}

impl Default for PreviewUiRuntime {
    fn default() -> Self {
        Self {
            mode: PresentationMode::Docked,
            context: ArtifactPreviewContext::Application,
            current: None,
            loading_title: None,
            loading_message: crate::i18n::tr("preview.preparing").as_ref().into(),
            loading_copy: crate::i18n::LocalizedText::message("preview.preparing"),
            detached: None,
            docked_virtual_layout: Rc::new(RefCell::new(MarkdownVirtualLayoutBridge::default())),
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
        self.open_source_in_context(source, ArtifactPreviewContext::Application);
    }

    pub fn open_pr_review_source(&self, source: ArtifactSource) {
        self.open_source_in_context(source, ArtifactPreviewContext::PrReviewSidePanel);
    }

    fn open_source_in_context(&self, source: ArtifactSource, context: ArtifactPreviewContext) {
        let title = source.title.clone();
        self.open_resolved_source(title, context, move || Ok(source));
    }

    fn open_resolved_source<F>(&self, title: String, context: ArtifactPreviewContext, resolve: F)
    where
        F: FnOnce() -> Result<ArtifactSource, usher_preview::PreviewError> + Send + 'static,
    {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let (generation, cancellation) = self.service.begin_request();
        set_preview_context(&ui, context);
        show_loading(&ui, title.clone());

        let progress_service = self.service.clone();
        let progress_ui = self.ui.clone();
        cancellation.set_progress_handler(move |progress| {
            let message = preparation_progress_text(progress);
            if !progress_service.is_current(generation) {
                return;
            }
            let progress_service = progress_service.clone();
            let progress_ui = progress_ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if !progress_service.is_current(generation) {
                    return;
                }
                if let Some(ui) = progress_ui.upgrade() {
                    update_loading_message(&ui, message);
                }
            });
        });

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
                let source = resolve()?;
                let prepared = registry.prepare(source, &cancellation)?;
                crate::memory_diagnostics::log_checkpoint("artifact_preview.provider_prepared");
                adapt_prepared_worker(prepared, &cancellation)
            })
            .await;
            crate::memory_diagnostics::log_checkpoint("artifact_preview.worker_prepared");

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
                    Err(error) => UiPayload::error(title, user_safe_error_key(&error)),
                };
                crate::memory_diagnostics::log_checkpoint("artifact_preview.ui_payload_converted");
                show_payload(&ui, payload);
            });
        });
    }

    pub fn open_file(&self, path: impl Into<PathBuf>, mime_type: Option<String>) {
        self.open_source(ArtifactSource::file(path, mime_type));
    }

    /// Opens a repository Markdown file in the PR-side Preview without trusting
    /// the model-provided path. Resolution and reading happen on the shared
    /// worker runtime; the canonical target must remain inside the canonical
    /// repository root.
    pub fn open_project_markdown(
        &self,
        project_root: impl Into<PathBuf>,
        relative_path: impl Into<PathBuf>,
    ) {
        let project_root = project_root.into();
        let relative_path =
            PathBuf::from(relative_path.into().to_string_lossy().replace('\\', "/"));
        let title = relative_path.to_string_lossy().into_owned();
        self.open_resolved_source(
            title,
            ArtifactPreviewContext::PrReviewSidePanel,
            move || resolve_project_markdown(&project_root, &relative_path),
        );
    }
}

fn resolve_project_markdown(
    project_root: &Path,
    relative_path: &Path,
) -> Result<ArtifactSource, usher_preview::PreviewError> {
    validate_repository_markdown_path(relative_path)?;
    let canonical_root = fs::canonicalize(project_root)
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    let file = open_repository_markdown_file(&canonical_root, relative_path)?;
    let bytes = read_bounded_regular_file(file, MAX_SOURCE_BYTES)?;
    let markdown =
        String::from_utf8(bytes).map_err(|_| usher_preview::PreviewError::InvalidUtf8)?;
    let title = relative_path.to_string_lossy().into_owned();
    Ok(ArtifactSource::inline_markdown(
        format!(
            "repository-markdown:{}:{}",
            canonical_root.display(),
            relative_path.display()
        ),
        title,
        markdown,
    ))
}

#[cfg(unix)]
fn open_repository_markdown_file(
    canonical_root: &Path,
    relative_path: &Path,
) -> Result<fs::File, usher_preview::PreviewError> {
    let root = open_repository_root(canonical_root)?;
    open_repository_markdown_at(&root, relative_path)
}

#[cfg(unix)]
fn open_repository_root(
    canonical_root: &Path,
) -> Result<std::os::fd::OwnedFd, usher_preview::PreviewError> {
    use rustix::fs::{Mode, OFlags, open};

    open(
        canonical_root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))
}

#[cfg(unix)]
fn open_repository_markdown_at(
    root: &std::os::fd::OwnedFd,
    relative_path: &Path,
) -> Result<fs::File, usher_preview::PreviewError> {
    use rustix::fs::{Mode, OFlags, openat};

    let mut current = root
        .try_clone()
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    let mut components = relative_path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(usher_preview::PreviewError::Io(
                "repository Markdown path contains an unsafe component".into(),
            ));
        };
        let final_component = components.peek().is_none();
        let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        if !final_component {
            flags |= OFlags::DIRECTORY;
        }
        current = openat(&current, name, flags, Mode::empty()).map_err(|error| {
            usher_preview::PreviewError::Io(format!(
                "repository Markdown path could not be opened without following links: {error}"
            ))
        })?;
    }
    Ok(fs::File::from(current))
}

#[cfg(target_os = "windows")]
fn open_repository_markdown_file(
    canonical_root: &Path,
    relative_path: &Path,
) -> Result<fs::File, usher_preview::PreviewError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(canonical_root)
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    let root_final = windows_final_path(&root)?;
    let file = fs::File::open(canonical_root.join(relative_path))
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    let target_final = windows_final_path(&file)?;
    if !windows_path_starts_with(&target_final, &root_final) {
        return Err(usher_preview::PreviewError::Io(
            "repository Markdown path escapes the project root".into(),
        ));
    }
    Ok(file)
}

#[cfg(target_os = "windows")]
fn windows_final_path(file: &fs::File) -> Result<PathBuf, usher_preview::PreviewError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let mut buffer = vec![0u16; 512];
    loop {
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len().min(u32::MAX as usize) as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(usher_preview::PreviewError::Io(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        if (length as usize) < buffer.len() {
            buffer.truncate(length as usize);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

#[cfg(target_os = "windows")]
fn windows_path_starts_with(path: &Path, root: &Path) -> bool {
    let mut path_components = path.components();
    root.components().all(|root_component| {
        path_components.next().is_some_and(|path_component| {
            path_component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_component.as_os_str().to_string_lossy())
        })
    })
}

#[cfg(not(any(unix, target_os = "windows")))]
fn open_repository_markdown_file(
    _canonical_root: &Path,
    _relative_path: &Path,
) -> Result<fs::File, usher_preview::PreviewError> {
    Err(usher_preview::PreviewError::Io(
        "secure repository Markdown opening is unsupported on this platform".into(),
    ))
}

fn read_bounded_regular_file(
    file: fs::File,
    limit: usize,
) -> Result<Vec<u8>, usher_preview::PreviewError> {
    let metadata = file
        .metadata()
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(usher_preview::PreviewError::Io(
            "repository Markdown target is not a regular file".into(),
        ));
    }
    if metadata.len() > limit as u64 {
        return Err(usher_preview::PreviewError::SourceTooLarge {
            bytes: metadata.len(),
            limit,
        });
    }

    let mut bytes = Vec::with_capacity(metadata.len().min(limit as u64) as usize);
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| usher_preview::PreviewError::Io(error.to_string()))?;
    if bytes.len() > limit {
        return Err(usher_preview::PreviewError::SourceTooLarge {
            bytes: bytes.len() as u64,
            limit,
        });
    }
    Ok(bytes)
}

fn validate_repository_markdown_path(
    relative_path: &Path,
) -> Result<(), usher_preview::PreviewError> {
    let path_text = relative_path.to_string_lossy();
    let valid_extension = relative_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        });
    let safe_components = !relative_path.as_os_str().is_empty()
        && !relative_path.is_absolute()
        && !path_text.contains(':')
        && path_text
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if safe_components && valid_extension {
        Ok(())
    } else {
        Err(usher_preview::PreviewError::Io(
            "repository Markdown path is not a safe relative Markdown file".into(),
        ))
    }
}

fn preparation_progress_text(progress: PreviewPreparationProgress) -> crate::i18n::LocalizedText {
    match progress {
        PreviewPreparationProgress::ParserLoading { display_name, .. } => {
            crate::i18n::LocalizedText::message_with(
                "preview.parser_loading",
                [("display_name", display_name)],
            )
        }
        PreviewPreparationProgress::ParserReady { display_name, .. } => {
            crate::i18n::LocalizedText::message_with(
                "preview.parser_ready",
                [("display_name", display_name)],
            )
        }
    }
}

#[cfg(test)]
fn preparation_progress_message(progress: PreviewPreparationProgress) -> String {
    preparation_progress_text(progress).resolve()
}

pub(crate) fn setup(ui: &MainWindow, settings: &PreviewSettings) -> ArtifactPreviewHandle {
    benchmark::setup(ui);
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
    let docked_virtual_layout = PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        runtime.docked_virtual_layout =
            Rc::new(RefCell::new(MarkdownVirtualLayoutBridge::from_environment()));
        runtime.docked_virtual_layout.clone()
    });
    let weak = ui.as_weak();
    let acknowledge = Arc::new(docked_virtual_layout.borrow().notification_acknowledger());
    docked_virtual_layout.borrow_mut().enable_worker(move || {
        let weak = weak.clone();
        let acknowledge = acknowledge.clone();
        let _ = slint::invoke_from_event_loop(move || {
            acknowledge();
            if let Some(ui) = weak.upgrade() {
                ui.set_artifact_preview_markdown_layout_epoch(
                    ui.get_artifact_preview_markdown_layout_epoch()
                        .wrapping_add(1),
                );
            }
        });
    });
    let layout_bridge = docked_virtual_layout.clone();
    ui.on_artifact_preview_layout_markdown_window(
        move |revision, blocks, scroll_offset, visible_height, content_width, font_family| {
            let started = std::time::Instant::now();
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
            benchmark::layout(started, &window);
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
        let http = url
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"));
        let https = url
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"));
        if !(http || https) {
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
    Markdown(
        Vec<UiPreparedMarkdownBlock>,
        Arc<Vec<measurement::MeasurementBlock>>,
    ),
    PlainText(String),
    Image(usher_preview::DecodedImage),
}

#[derive(Clone)]
struct UiPreparedMarkdownBlock {
    kind: ArtifactPreviewBlockKind,
    styled: slint::StyledText,
    list_marker: String,
    list_depth: i32,
    code_lines: Arc<Vec<UiPreparedCodeLine>>,
    code_language: String,
    plain_text: String,
    layout_spans: Vec<ArtifactPreviewInlineSpan>,
}

#[derive(Clone)]
struct UiPreparedCodeLine {
    tokens: Vec<UiPreparedCodeToken>,
}

#[derive(Clone)]
struct UiPreparedCodeToken {
    text: String,
    light_color: [u8; 4],
    dark_color: [u8; 4],
    bold: bool,
    italic: bool,
}

impl UiPreparedPayload {
    fn into_ui_payload(self) -> UiPayload {
        let (kind, blocks, plain_text, image) = match self.content {
            UiPreparedContent::Markdown(blocks, measurements) => (
                ArtifactPreviewKind::Markdown,
                ModelRc::new(projection_model::MarkdownModel::new(blocks, measurements)),
                "".into(),
                slint::Image::default(),
            ),
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
            error_key: None,
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
                content: {
                    let measurements = Arc::new(
                        blocks
                            .iter()
                            .map(|block| measurement::MeasurementBlock {
                                kind: block.kind,
                                plain_text: block.plain_text.clone(),
                                list_depth: block.list_depth,
                                code_language: block.code_language.clone(),
                                layout_spans: block.layout_spans.clone(),
                            })
                            .collect(),
                    );
                    UiPreparedContent::Markdown(blocks, measurements)
                },
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
            list_marker: self.list_marker.into(),
            list_depth: self.list_depth,
            code_lines: ModelRc::new(projection_model::CodeLinesModel::new(self.code_lines)),
            code_language: self.code_language.into(),
            plain_text: self.plain_text.into(),
            layout_spans: ModelRc::new(Rc::new(VecModel::from(self.layout_spans))),
        }
    }
}

fn adapt_markdown_block(block: &CoreMarkdownBlock) -> UiPreparedMarkdownBlock {
    let (display_block, list_marker, list_depth) = preview_list_presentation(block);
    let mut byte_offset = 0usize;
    let layout_spans = display_block
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
                    MarkdownInlineKind::Strikethrough => ArtifactPreviewInlineKind::Strikethrough,
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
            CoreBlockKind::Heading4 => ArtifactPreviewBlockKind::Heading4,
            CoreBlockKind::Heading5 => ArtifactPreviewBlockKind::Heading5,
            CoreBlockKind::Heading6 => ArtifactPreviewBlockKind::Heading6,
            CoreBlockKind::Paragraph => ArtifactPreviewBlockKind::Paragraph,
            CoreBlockKind::ListItem => ArtifactPreviewBlockKind::ListItem,
            CoreBlockKind::Quote => ArtifactPreviewBlockKind::Quote,
            CoreBlockKind::CodeBlock => ArtifactPreviewBlockKind::CodeBlock,
            CoreBlockKind::Rule => ArtifactPreviewBlockKind::Rule,
            CoreBlockKind::TableRow => ArtifactPreviewBlockKind::TableRow,
        },
        styled: styled_text_for(&display_block),
        list_marker,
        list_depth,
        code_lines: Arc::new(prepared_code_lines(block)),
        code_language: block.code_language.clone(),
        plain_text: display_block.text,
        layout_spans,
    }
}

fn preview_list_presentation(block: &CoreMarkdownBlock) -> (CoreMarkdownBlock, String, i32) {
    let mut display = block.clone();
    if block.kind != CoreBlockKind::ListItem {
        return (display, String::new(), 0);
    }

    let Some(first) = display.spans.first_mut() else {
        return (display, "•".to_string(), 0);
    };
    let Some((prefix_bytes, marker, depth)) = split_preview_list_prefix(&first.text) else {
        return (display, "•".to_string(), 0);
    };
    first.text.replace_range(..prefix_bytes, "");
    if first.text.is_empty() {
        display.spans.remove(0);
    }
    display.text = display
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    (display, marker, depth)
}

fn split_preview_list_prefix(text: &str) -> Option<(usize, String, i32)> {
    let leading_spaces = text.bytes().take_while(|byte| *byte == b' ').count();
    let depth = (leading_spaces / 2).min(i32::MAX as usize) as i32;
    let rest = &text[leading_spaces..];
    let (marker_bytes, mut marker) = if rest.starts_with("• ") {
        ("• ".len(), "•".to_string())
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !rest[digits..].starts_with(". ") {
            return None;
        }
        (digits + 2, rest[..=digits].to_string())
    };
    let mut consumed = leading_spaces + marker_bytes;
    let after_marker = &text[consumed..];
    if after_marker.starts_with("[x] ") || after_marker.starts_with("[X] ") {
        marker = "✓".to_string();
        consumed += 4;
    } else if after_marker.starts_with("[ ] ") {
        marker = "○".to_string();
        consumed += 4;
    }
    Some((consumed, marker, depth))
}

fn prepared_code_lines(block: &CoreMarkdownBlock) -> Vec<UiPreparedCodeLine> {
    if block.kind != CoreBlockKind::CodeBlock {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut tokens = Vec::new();
    for span in &block.code_spans {
        for segment in span.text.split_inclusive('\n') {
            let text = segment.strip_suffix('\n').unwrap_or(segment);
            if !text.is_empty() {
                tokens.push(UiPreparedCodeToken {
                    text: text.to_owned(),
                    light_color: span.light_color,
                    dark_color: span.dark_color,
                    bold: span.bold,
                    italic: span.italic,
                });
            }
            if segment.ends_with('\n') {
                lines.push(UiPreparedCodeLine {
                    tokens: std::mem::take(&mut tokens),
                });
            }
        }
    }
    if !tokens.is_empty() || lines.is_empty() {
        lines.push(UiPreparedCodeLine { tokens });
    }
    lines
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
        CoreBlockKind::Heading1
            | CoreBlockKind::Heading2
            | CoreBlockKind::Heading3
            | CoreBlockKind::Heading4
            | CoreBlockKind::Heading5
            | CoreBlockKind::Heading6
    );
    if heading {
        markdown.push_str("**");
    }
    for span in &block.spans {
        match span.kind {
            MarkdownInlineKind::Text => markdown.push_str(&escape_inline_markdown(&span.text)),
            MarkdownInlineKind::Strong => {
                markdown.push_str("**");
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("**");
            }
            MarkdownInlineKind::Emphasis => {
                markdown.push('*');
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push('*');
            }
            MarkdownInlineKind::Strikethrough => {
                markdown.push_str("~~");
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("~~");
            }
            MarkdownInlineKind::Code => push_inline_code_markdown(&mut markdown, &span.text),
            MarkdownInlineKind::Link => {
                markdown.push('[');
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("](");
                markdown.push_str(&span.href.replace(')', "\\)"));
                markdown.push(')');
            }
        }
    }
    if heading {
        markdown.push_str("**");
    }
    slint::StyledText::from_markdown(&markdown)
        .unwrap_or_else(|_| slint::StyledText::from_plain_text(&block.text))
}

fn push_inline_code_markdown(markdown: &mut String, text: &str) {
    let fence_length = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or_default()
        .saturating_add(1);
    let fence = "`".repeat(fence_length);
    let pad = text.starts_with('`') || text.ends_with('`');
    markdown.push_str(&fence);
    if pad {
        markdown.push(' ');
    }
    markdown.push_str(text);
    if pad {
        markdown.push(' ');
    }
    markdown.push_str(&fence);
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
        runtime.loading_copy = crate::i18n::LocalizedText::message("preview.preparing");
        runtime.loading_message = runtime.loading_copy.resolve().into();
        present_runtime(ui, &mut runtime);
    });
}

fn set_preview_context(ui: &MainWindow, context: ArtifactPreviewContext) {
    PREVIEW_UI.with(|runtime| runtime.borrow_mut().context = context);
    ui.set_artifact_preview_pr_side_context(context == ArtifactPreviewContext::PrReviewSidePanel);
}

fn update_loading_message(ui: &MainWindow, message: crate::i18n::LocalizedText) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        if runtime.loading_title.is_none() || runtime.current.is_some() {
            return;
        }
        runtime.loading_message = message.resolve().into();
        runtime.loading_copy = message;
        present_runtime(ui, &mut runtime);
    });
}

pub(crate) fn sync_language(ui: &MainWindow) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        runtime.loading_message = runtime.loading_copy.resolve().into();
        if let Some(payload) = runtime.current.as_mut()
            && let Some(key) = payload.error_key
        {
            payload.error = crate::i18n::tr(key).into_owned().into();
        }
        if runtime.loading_title.is_some() || runtime.current.is_some() {
            present_runtime(ui, &mut runtime);
        }
    });
}

fn show_payload(ui: &MainWindow, payload: UiPayload) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        runtime.loading_title = None;
        runtime.loading_copy = crate::i18n::LocalizedText::message("preview.preparing");
        runtime.loading_message = runtime.loading_copy.resolve().into();
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

#[cfg(all(feature = "ai-code-review", any(test, feature = "test-fixtures")))]
pub(crate) fn set_memory_benchmark_presentation_mode(ui: &MainWindow, detached: bool) {
    if !crate::memory_diagnostics::diagnostics_enabled() {
        return;
    }
    let mode = if detached {
        PresentationMode::Detached
    } else {
        PresentationMode::Docked
    };
    AppState::get(ui).set_artifact_preview_presentation_mode(i32::from(detached));
    switch_mode(ui, mode);
}

fn present_runtime(ui: &MainWindow, runtime: &mut PreviewUiRuntime) {
    ui.set_artifact_preview_pr_side_context(
        runtime.context == ArtifactPreviewContext::PrReviewSidePanel,
    );
    let has_content = runtime.current.is_some() || runtime.loading_title.is_some();
    if !has_content {
        if runtime.mode == PresentationMode::Docked {
            begin_main_host_close(ui, runtime.docked_virtual_layout.clone());
        } else {
            clear_main_host_immediately(ui, &runtime.docked_virtual_layout);
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
                apply_loading_to_main(ui, title, &runtime.loading_message);
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
                apply_loading_to_window(&window, title, &runtime.loading_message);
            }
            center_detached_window(ui, &window);
            if let Err(error) = window.show() {
                log::error!("[ArtifactPreview] Failed to show detached window: {error}");
            } else {
                install_detached_live_resize_bridge(&window);
            }
            clear_main_host_immediately(ui, &runtime.docked_virtual_layout);
        }
    }
}

fn install_detached_live_resize_bridge(window: &ArtifactPreviewWindow) {
    // winit creates the NSWindow asynchronously after Slint's show() request.
    // Try immediately, then retry on bounded event-loop turns. The platform
    // adapter is idempotent once it observes the current native window.
    crate::platform::install_artifact_preview_live_resize(window);
    crate::platform::install_artifact_preview_titlebar_actions(window);
    for delay in [30, 120] {
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(delay), move || {
            if let Some(window) = weak.upgrade() {
                crate::platform::install_artifact_preview_live_resize(&window);
                crate::platform::install_artifact_preview_titlebar_actions(&window);
            }
        });
    }
}

fn ensure_detached_window(
    ui: &MainWindow,
    runtime: &mut PreviewUiRuntime,
) -> Option<ArtifactPreviewWindow> {
    let dark_mode = AppState::get(ui).get_effective_dark_mode();
    if let Some(window) = runtime.detached.as_ref() {
        window.set_effective_dark_mode(dark_mode);
        return Some(window.clone_strong());
    }
    let window = match ArtifactPreviewWindow::new() {
        Ok(window) => window,
        Err(error) => {
            log::error!("[ArtifactPreview] Failed to create detached window: {error}");
            return None;
        }
    };
    window.set_effective_dark_mode(dark_mode);
    window.set_application_font_family(ui.get_application_font_family());
    window.set_code_font_family(ui.get_workflow_pr_review_code_font_family());
    window.set_performance_optimized(ui.get_artifact_preview_performance_optimized());
    window.set_native_titlebar_actions(cfg!(target_os = "macos"));
    let detached_virtual_layout =
        Rc::new(RefCell::new(MarkdownVirtualLayoutBridge::from_environment()));
    let weak = window.as_weak();
    let acknowledge = Arc::new(detached_virtual_layout.borrow().notification_acknowledger());
    detached_virtual_layout.borrow_mut().enable_worker(move || {
        let weak = weak.clone();
        let acknowledge = acknowledge.clone();
        let _ = slint::invoke_from_event_loop(move || {
            acknowledge();
            if let Some(window) = weak.upgrade() {
                window
                    .set_markdown_layout_epoch(window.get_markdown_layout_epoch().wrapping_add(1));
            }
        });
    });
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

pub(crate) fn sync_detached_effective_theme(dark_mode: bool) {
    PREVIEW_UI.with(|runtime| {
        if let Some(window) = runtime.borrow().detached.as_ref() {
            window.set_effective_dark_mode(dark_mode);
        }
    });
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

fn apply_loading_to_main(
    ui: &MainWindow,
    title: &slint::SharedString,
    message: &slint::SharedString,
) {
    HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel);
    clear_main_payload(ui);
    ui.set_artifact_preview_title(title.clone());
    ui.set_artifact_preview_loading_message(message.clone());
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
    crate::platform::install_artifact_preview_titlebar_actions(window);
}

fn next_layout_revision() -> i32 {
    (HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel) as i32).wrapping_add(1)
}

fn apply_loading_to_window(
    window: &ArtifactPreviewWindow,
    title: &slint::SharedString,
    message: &slint::SharedString,
) {
    clear_window_payload(window);
    window.set_document_title(title.clone());
    window.set_loading_message(message.clone());
    window.set_loading(true);
    crate::platform::install_artifact_preview_titlebar_actions(window);
}

fn clear_hosts(ui: &MainWindow) {
    PREVIEW_UI.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        let animate_docked_close = runtime.mode == PresentationMode::Docked;
        runtime.current = None;
        runtime.loading_title = None;
        runtime.loading_copy = crate::i18n::LocalizedText::message("preview.preparing");
        runtime.loading_message = runtime.loading_copy.resolve().into();
        runtime.context = ArtifactPreviewContext::Application;
        if animate_docked_close {
            begin_main_host_close(ui, runtime.docked_virtual_layout.clone());
        } else {
            clear_main_host_immediately(ui, &runtime.docked_virtual_layout);
        }
        if let Some(window) = runtime.detached.take() {
            clear_window_payload(&window);
            let _ = window.hide();
        }
        ui.set_artifact_preview_pr_side_context(false);
    });
}

fn begin_main_host_close(
    ui: &MainWindow,
    docked_virtual_layout: Rc<RefCell<MarkdownVirtualLayoutBridge>>,
) {
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
            docked_virtual_layout.borrow_mut().reset_document();
        }
    });
}

fn clear_main_host_immediately(
    ui: &MainWindow,
    docked_virtual_layout: &Rc<RefCell<MarkdownVirtualLayoutBridge>>,
) {
    HOST_VISUAL_GENERATION.fetch_add(1, Ordering::AcqRel);
    ui.set_artifact_preview_visible(false);
    ui.set_artifact_preview_loading(false);
    clear_main_payload(ui);
    docked_virtual_layout.borrow_mut().reset_document();
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

fn user_safe_error_key(error: &usher_preview::PreviewError) -> &'static str {
    match error {
        usher_preview::PreviewError::Unsupported => "preview.error.unsupported",
        usher_preview::PreviewError::SourceTooLarge { .. } => "preview.error.too_large",
        usher_preview::PreviewError::Cancelled => "preview.error.cancelled",
        usher_preview::PreviewError::InvalidUtf8 => "preview.error.invalid_utf8",
        usher_preview::PreviewError::Io(_) => "preview.error.io",
        usher_preview::PreviewError::Decode(_) => "preview.error.decode",
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
    const TYPESCRIPT_FIXTURE: &str = "artifact-preview-markdown-typescript-256k-371-blocks-v1";
    let typescript = std::env::var("USHER_MEMORY_FIXTURE").as_deref() == Ok(TYPESCRIPT_FIXTURE);
    memory_benchmark_markdown_fixture_for(if typescript { "typescript" } else { "rust" })
}

fn memory_benchmark_markdown_fixture_for(language: &str) -> String {
    const TARGET_BYTES: usize = 256 * 1024;
    const SECTION_PREFIX: &str = "## Result\n\n- **Passed**: `cargo test --lib --locked`\n- [Report](https://example.invalid/report)\n\n> Width-independent projection keeps Chat resize cheap.\n\n```";
    const RUST_CODE_LINE: &str = "let preview = registry.prepare(source, &cancellation)?;\n";
    const TYPESCRIPT_CODE_LINE: &str = "const preview = registry.prepare(source, cancellation);\n";
    const SECTION_END: &str =
        "```\n\n| Metric | Value |\n| --- | ---: |\n| model commits | 2 |\n| projections | 1 |\n\n";
    let code_line = if language == "typescript" {
        TYPESCRIPT_CODE_LINE
    } else {
        RUST_CODE_LINE
    };
    let section_start = format!("{SECTION_PREFIX}{language}\n");
    let mut markdown = String::with_capacity(TARGET_BYTES);
    markdown.push_str("# Artifact preview benchmark\n\n");
    let section_bytes = section_start.len() + code_line.len() * 96 + SECTION_END.len();
    while markdown.len() + section_bytes <= TARGET_BYTES {
        markdown.push_str(&section_start);
        for _ in 0..96 {
            markdown.push_str(code_line);
        }
        markdown.push_str(SECTION_END);
    }
    markdown.push_str("## Appendix\n\n");
    markdown.push_str(&"x".repeat(TARGET_BYTES - markdown.len()));
    markdown
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn repository_fixture_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "usher-artifact-preview-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn presentation_mode_maps_unknown_values_to_docked() {
        assert_eq!(PresentationMode::from_index(-1), PresentationMode::Docked);
        assert_eq!(PresentationMode::from_index(0), PresentationMode::Docked);
        assert_eq!(PresentationMode::from_index(1), PresentationMode::Detached);
        assert_eq!(PresentationMode::from_index(2), PresentationMode::Docked);
    }

    #[test]
    fn detached_window_receives_initial_reused_and_live_effective_theme() {
        i_slint_backend_testing::init_no_event_loop();
        let initial_light = ArtifactPreviewWindow::new().expect("initial light preview window");
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
        assert!(!initial_light.get_effective_dark_mode());
        assert!(initial_light.get_palette_synchronized());
        assert_eq!(AppState::get(&initial_light).get_theme_mode(), 0);
        assert!(!AppState::get(&initial_light).get_effective_dark_mode());

        let ui = MainWindow::new().expect("main window");
        let source_state = AppState::get(&ui);
        source_state.set_theme_mode(1);
        source_state.set_effective_dark_mode(true);

        let detached = PREVIEW_UI.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            ensure_detached_window(&ui, &mut runtime).expect("detached preview window")
        });
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
        detached.set_markdown_layout_revision(42);
        assert_eq!(
            detached.get_native_titlebar_actions(),
            cfg!(target_os = "macos")
        );
        assert!(detached.get_effective_dark_mode());
        assert!(detached.get_palette_synchronized());
        assert_eq!(AppState::get(&detached).get_theme_mode(), 1);
        assert!(AppState::get(&detached).get_effective_dark_mode());

        source_state.set_theme_mode(2);
        source_state.set_effective_dark_mode(false);
        let reused = PREVIEW_UI.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            ensure_detached_window(&ui, &mut runtime).expect("reused detached preview window")
        });
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
        assert!(!reused.get_effective_dark_mode());
        assert!(reused.get_palette_synchronized());
        assert_eq!(AppState::get(&detached).get_theme_mode(), 0);
        assert!(!AppState::get(&detached).get_effective_dark_mode());
        assert_eq!(detached.get_markdown_layout_revision(), 42);

        sync_detached_effective_theme(true);
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);
        assert!(detached.get_effective_dark_mode());
        assert!(detached.get_palette_synchronized());
        assert_eq!(AppState::get(&detached).get_theme_mode(), 1);
        assert!(AppState::get(&detached).get_effective_dark_mode());
        assert_eq!(detached.get_markdown_layout_revision(), 42);

        PREVIEW_UI.with(|runtime| {
            runtime.borrow_mut().detached.take();
        });
    }

    #[test]
    fn parser_progress_messages_name_the_active_language() {
        assert_eq!(
            preparation_progress_message(PreviewPreparationProgress::ParserLoading {
                language: "typescript".to_string(),
                loader_id: "typescript",
                display_name: "TypeScript".to_string(),
            }),
            "Loading the TypeScript Parser…"
        );
        assert_eq!(
            preparation_progress_message(PreviewPreparationProgress::ParserReady {
                language: "typescript".to_string(),
                loader_id: "typescript",
                display_name: "TypeScript".to_string(),
            }),
            "The TypeScript Parser is ready. Preparing preview…"
        );
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
    fn inline_code_markdown_does_not_reintroduce_visible_escape_characters() {
        let mut markdown = String::new();
        push_inline_code_markdown(&mut markdown, r"i-slint-backend-* [dev-dependencies]");
        assert_eq!(markdown, "`i-slint-backend-* [dev-dependencies]`");

        markdown.clear();
        push_inline_code_markdown(&mut markdown, "call(`value`)");
        assert_eq!(markdown, "``call(`value`)``");
    }

    #[test]
    fn repository_markdown_resolution_reads_only_contained_files() {
        let root = repository_fixture_root("contained");
        let nested = root.join("docs/specs");
        fs::create_dir_all(&nested).expect("fixture directories");
        fs::write(nested.join("行为.md"), "# Fixture\n\nContained.").expect("fixture markdown");

        let source = resolve_project_markdown(&root, Path::new("docs/specs/行为.md"))
            .expect("contained Markdown should resolve");
        assert_eq!(source.title, "docs/specs/行为.md");
        assert!(matches!(source.body, ArtifactBody::Text(ref text) if text.contains("Contained")));

        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[test]
    fn repository_markdown_path_rejects_unsafe_or_non_markdown_targets() {
        for path in [
            "",
            "../outside.md",
            "/absolute.md",
            "docs/./guide.md",
            "docs/readme.txt",
        ] {
            assert!(
                validate_repository_markdown_path(Path::new(path)).is_err(),
                "unexpectedly accepted {path:?}"
            );
        }
        assert!(validate_repository_markdown_path(Path::new("AGENTS.md")).is_ok());
        assert!(validate_repository_markdown_path(Path::new("docs/guide.MARKDOWN")).is_ok());
    }

    #[test]
    fn repository_markdown_reader_enforces_a_hard_handle_read_limit() {
        let root = repository_fixture_root("bounded-read");
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join("large.md");
        fs::write(&path, vec![b'x'; 65]).expect("fixture markdown");

        let file = fs::File::open(&path).expect("fixture handle");
        let error = read_bounded_regular_file(file, 64)
            .expect_err("oversized opened file must be rejected");
        assert!(matches!(
            error,
            usher_preview::PreviewError::SourceTooLarge {
                bytes: 65,
                limit: 64
            }
        ));

        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[test]
    fn repository_markdown_resolution_rejects_invalid_utf8() {
        let root = repository_fixture_root("invalid-utf8");
        fs::create_dir_all(&root).expect("fixture root");
        fs::write(root.join("invalid.md"), [0xff, 0xfe, 0xfd]).expect("fixture bytes");

        let error = resolve_project_markdown(&root, Path::new("invalid.md"))
            .expect_err("invalid UTF-8 Markdown must be rejected");
        assert!(matches!(error, usher_preview::PreviewError::InvalidUtf8));

        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn repository_markdown_resolution_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = repository_fixture_root("symlink-root");
        let outside = repository_fixture_root("symlink-outside");
        fs::create_dir_all(&root).expect("root fixture");
        fs::create_dir_all(&outside).expect("outside fixture");
        let outside_file = outside.join("outside.md");
        fs::write(&outside_file, "# Outside").expect("outside markdown");
        symlink(&outside_file, root.join("escape.md")).expect("fixture symlink");

        let error = resolve_project_markdown(&root, Path::new("escape.md"))
            .expect_err("symlink escape must be rejected");
        assert!(error.to_string().contains("without following links"));

        fs::remove_dir_all(root).expect("root cleanup");
        fs::remove_dir_all(outside).expect("outside cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn repository_markdown_open_rejects_replacement_after_root_is_open() {
        use std::os::unix::fs::symlink;

        let root = repository_fixture_root("replacement-root");
        let outside = repository_fixture_root("replacement-outside");
        fs::create_dir_all(&root).expect("root fixture");
        fs::create_dir_all(&outside).expect("outside fixture");
        let target = root.join("guide.md");
        let outside_file = outside.join("outside.md");
        fs::write(&target, "# Safe").expect("initial markdown");
        fs::write(&outside_file, "# Outside").expect("outside markdown");

        let canonical_root = fs::canonicalize(&root).expect("canonical fixture root");
        let root_handle = open_repository_root(&canonical_root).expect("repository root handle");
        validate_repository_markdown_path(Path::new("guide.md")).expect("validated path");
        fs::remove_file(&target).expect("replace validated target");
        symlink(&outside_file, &target).expect("replacement symlink");

        let error = open_repository_markdown_at(&root_handle, Path::new("guide.md"))
            .expect_err("descriptor-relative open must reject the replacement");
        assert!(error.to_string().contains("without following links"));

        fs::remove_dir_all(root).expect("root cleanup");
        fs::remove_dir_all(outside).expect("outside cleanup");
    }

    #[test]
    fn markdown_lists_reach_preview_with_hanging_markers_and_task_symbols() {
        let document = usher_preview::parse_markdown_structure(
            "1. first\n2. second\n\n- [x] complete\n- [ ] pending",
            "Lists",
        );
        let blocks = document
            .blocks
            .iter()
            .map(|block| adapt_markdown_block(block).into_ui_block())
            .collect::<Vec<_>>();

        assert_eq!(blocks[0].list_marker, "1.");
        assert_eq!(blocks[0].plain_text, "first");
        assert_eq!(blocks[1].list_marker, "2.");
        assert_eq!(blocks[1].plain_text, "second");
        assert_eq!(blocks[2].list_marker, "✓");
        assert_eq!(blocks[2].plain_text, "complete");
        assert_eq!(blocks[3].list_marker, "○");
        assert_eq!(blocks[3].plain_text, "pending");
    }

    #[test]
    fn real_typescript_markdown_reaches_slint_with_distinct_token_colors() {
        let document = usher_preview::parse_markdown(
            "```typescript\nimport { WeCom } from '@picc/jsbridge';\nawait WeCom.register({ appId: 'your-app-id' });\n```",
            "Code",
        );
        let block = document
            .blocks
            .iter()
            .find(|block| block.kind == CoreBlockKind::CodeBlock)
            .expect("typescript code block");
        let ui_block = adapt_markdown_block(block).into_ui_block();
        let mut rendered_lines = Vec::new();
        let mut light_colors = std::collections::HashSet::new();

        for line in ui_block.code_lines.iter() {
            let mut rendered_line = String::new();
            for token in line.tokens.iter() {
                rendered_line.push_str(token.text.as_str());
                let color = token.light_color.to_argb_u8();
                light_colors.insert((color.alpha, color.red, color.green, color.blue));
            }
            rendered_lines.push(rendered_line);
        }

        assert_eq!(rendered_lines.join("\n"), block.text);
        assert!(
            light_colors.len() >= 3,
            "Slint model lost TypeScript token colors: {light_colors:?}"
        );
    }

    #[test]
    fn memory_benchmark_fixtures_have_a_fixed_size() {
        for language in ["rust", "typescript"] {
            let fixture = memory_benchmark_markdown_fixture_for(language);
            assert_eq!(fixture.len(), 256 * 1024);
            assert!(fixture.contains(&format!("```{language}\n")));
        }
    }
}
