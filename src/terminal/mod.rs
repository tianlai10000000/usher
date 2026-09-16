mod benchmark;
mod diagnostics;
mod input;
mod io;
mod lifecycle;
mod refresh;
mod render;
mod session;

use crate::memory_diagnostics::process_memory_snapshot;
use crate::{MainWindow, Route, TerminalLine, TerminalSpan};
use arboard::Clipboard;
use diagnostics::{format_bytes_for_log, route_label, sanitize_for_log, terminal_line_stats};
use input::map_key_to_bytes;
use refresh::next_refresh_delay;
use render::{RenderedTerminalLine, TerminalCursorState, TerminalRenderState};
use session::{PtySession, SessionEvent};
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::cell::RefCell;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

type SharedTerminalState = Arc<Mutex<TerminalDrawerState>>;
static TERMINAL_SHARED_STATE: OnceLock<SharedTerminalState> = OnceLock::new();
static TERMINAL_UI_WEAK: OnceLock<slint::Weak<MainWindow>> = OnceLock::new();
static TERMINAL_VERBOSE_LOGGING: AtomicBool = AtomicBool::new(false);

struct TerminalUiModels {
    lines_model: Rc<VecModel<TerminalLine>>,
    span_models: Vec<Rc<VecModel<TerminalSpan>>>,
    cached_lines: Vec<RenderedTerminalLine>,
}

thread_local! {
    static TERMINAL_UI_MODELS: RefCell<Option<TerminalUiModels>> = const { RefCell::new(None) };
}

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 120;
const ENABLE_TERMINAL_SPAN_RENDER: bool = true;
const TERMINAL_REFRESH_FRAME_INTERVAL: Duration = Duration::from_millis(16);

fn default_scrollback() -> usize {
    if std::env::var("SLINT_LIVE_PREVIEW").ok().as_deref() == Some("1") {
        500
    } else {
        1_000
    }
}

struct TerminalDrawerState {
    project_key: String,
    project_name: String,
    cwd: String,
    drawer_visible: bool,
    active_session_id: Option<u64>,
    next_session_id: u64,
    selection_mode: bool,
    selection_output: String,
    selection_output_line_count: i32,
    selection_output_prefix: String,
    selection_notice_key: Option<&'static str>,
    output_dirty: bool,
    lines: Vec<RenderedTerminalLine>,
    scrollback_offset: usize,
    max_scrollback: usize,
    max_scrollback_dirty: bool,
    cursor: TerminalCursorState,
    running: bool,
    connected: bool,
    input_failed: bool,
    rows: u16,
    cols: u16,
    dirty: bool,
    refresh_scheduled: bool,
    last_refresh_applied_at: Option<Instant>,
    last_diagnostics_log: Option<Instant>,
    last_pty_output_log: Option<Instant>,
    suppressed_pty_output_chunks: usize,
    suppressed_pty_output_bytes: usize,
    render: TerminalRenderState,
    session: Option<PtySession>,
    retirement: lifecycle::SessionRetirement,
}

impl Default for TerminalDrawerState {
    fn default() -> Self {
        let render = TerminalRenderState::new(DEFAULT_ROWS, DEFAULT_COLS, default_scrollback());
        Self {
            project_key: String::new(),
            project_name: String::new(),
            cwd: default_terminal_cwd(),
            drawer_visible: false,
            active_session_id: None,
            next_session_id: 1,
            selection_mode: false,
            selection_output: terminal_notice_line("terminal.ready"),
            selection_output_line_count: 1,
            selection_output_prefix: String::new(),
            selection_notice_key: Some("terminal.ready"),
            output_dirty: false,
            lines: default_terminal_lines(),
            scrollback_offset: 0,
            max_scrollback: 0,
            max_scrollback_dirty: false,
            cursor: TerminalCursorState::default(),
            running: false,
            connected: false,
            input_failed: false,
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            dirty: false,
            refresh_scheduled: false,
            last_refresh_applied_at: None,
            last_diagnostics_log: None,
            last_pty_output_log: None,
            suppressed_pty_output_chunks: 0,
            suppressed_pty_output_bytes: 0,
            render,
            session: None,
            retirement: lifecycle::SessionRetirement::default(),
        }
    }
}

pub fn setup_terminal(ui: &MainWindow) {
    TERMINAL_VERBOSE_LOGGING.store(
        crate::settings::load_settings()
            .diagnostics
            .terminal_verbose_logging,
        Ordering::Relaxed,
    );
    let shared_state = Arc::new(Mutex::new(TerminalDrawerState::default()));
    let _ = TERMINAL_SHARED_STATE.set(shared_state.clone());
    let _ = TERMINAL_UI_WEAK.set(ui.as_weak());
    initialize_ui(ui, &shared_state);
    bind_terminal(ui, shared_state.clone());
    crate::shutdown::register_async_drain("terminal_sessions", move || {
        Box::pin(lifecycle::drain(shared_state))
    });
    benchmark::setup(ui);
}

pub fn shutdown_terminal() {
    let Some(shared_state) = TERMINAL_SHARED_STATE.get() else {
        return;
    };
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    log::info!("[Terminal] Shutdown requested, terminating active PTY session");
    guard.retirement.closing = true;
    terminate_session_locked(&mut guard);
    reset_terminal_view_state(&mut guard);
}

pub fn set_dark_mode(ui: &MainWindow, is_dark: bool) {
    render::set_dark_mode(is_dark);

    let Some(shared_state) = TERMINAL_SHARED_STATE.get() else {
        return;
    };
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };

    let mut rendered_lines = Vec::new();
    guard.render.rendered_lines_into(&mut rendered_lines);
    guard.lines = rendered_lines;
    guard.dirty = true;
    drop(guard);

    initialize_ui(ui, shared_state);
    schedule_refresh(ui.as_weak(), shared_state.clone());
}

pub fn set_verbose_logging(enabled: bool) {
    TERMINAL_VERBOSE_LOGGING.store(enabled, Ordering::Relaxed);
}

pub fn sync_language(ui: &MainWindow) {
    let Some(shared_state) = TERMINAL_SHARED_STATE.get() else {
        return;
    };
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    let title = terminal_title_for_project(guard.project_name.as_str());
    let notice_changed = guard.selection_notice_key.is_some();
    if notice_changed {
        refresh_selection_notice(&mut guard);
    }
    let output = guard.selection_output.clone();
    let line_count = guard.selection_output_line_count;
    drop(guard);
    ui.set_shell_drawer_title(title.into());
    if notice_changed {
        ui.set_shell_drawer_output(output.into());
        ui.set_shell_drawer_output_line_count(line_count);
    }
}

pub fn log_page_memory_diag(
    route: Route,
    width_px: i32,
    height_px: i32,
    terminal_visible: bool,
    scale_factor: f32,
) {
    if !terminal_verbose_logging_enabled() {
        return;
    }

    let memory = process_memory_snapshot();
    let terminal_state = TERMINAL_SHARED_STATE
        .get()
        .and_then(|shared| shared.lock().ok())
        .map(|guard| {
            (
                guard.rows,
                guard.cols,
                guard.lines.len(),
                terminal_line_stats(&guard.lines),
                guard.scrollback_offset,
                guard.max_scrollback,
                guard.render.scrollback_capacity(),
            )
        });

    if let Some((
        rows,
        cols,
        line_count,
        (span_count, span_text_bytes),
        scrollback_offset,
        max_scrollback,
        scrollback_capacity,
    )) = terminal_state
    {
        log::info!(
            "[Terminal][Diag] reason=page_window, route={}, width_px={}, height_px={}, scale_factor={:.2}, terminal_visible={}, rows={}, cols={}, lines={}, spans={}, span_text_bytes={}, scrollback_offset={}, max_scrollback={}, scrollback_capacity={}, rss_current_bytes={}",
            route_label(route),
            width_px,
            height_px,
            scale_factor,
            terminal_visible,
            rows,
            cols,
            line_count,
            span_count,
            span_text_bytes,
            scrollback_offset,
            max_scrollback,
            scrollback_capacity,
            memory.rss_current_bytes,
        );
    } else {
        log::info!(
            "[Terminal][Diag] reason=page_window, route={}, width_px={}, height_px={}, scale_factor={:.2}, terminal_visible={}, terminal_state=uninitialized, rss_current_bytes={}",
            route_label(route),
            width_px,
            height_px,
            scale_factor,
            terminal_visible,
            memory.rss_current_bytes,
        );
    }
}

fn terminal_verbose_logging_enabled() -> bool {
    TERMINAL_VERBOSE_LOGGING.load(Ordering::Relaxed)
}

pub fn diag_logging_enabled() -> bool {
    terminal_verbose_logging_enabled()
}

pub fn reset_terminal_for_project(
    ui: &MainWindow,
    project_key: &str,
    cwd: &str,
    preserve_state: bool,
) {
    let Some(shared_state) = TERMINAL_SHARED_STATE.get() else {
        return;
    };
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };

    if preserve_state && guard.project_key == project_key {
        return;
    }

    terminate_session_locked(&mut guard);
    guard.project_key = project_key.to_string();
    guard.cwd = if cwd.trim().is_empty() {
        default_terminal_cwd()
    } else {
        cwd.trim().to_string()
    };
    guard.drawer_visible = false;
    reset_terminal_view_state(&mut guard);
    drop(guard);

    ui.set_shell_drawer_visible(false);
    ui.set_shell_drawer_input_text("".into());
    ui.set_shell_drawer_height(0.0);
    ui.set_shell_drawer_fullscreen(false);
    initialize_ui(ui, shared_state);
    schedule_refresh(ui.as_weak(), shared_state.clone());
}

pub fn open_terminal_for_project(
    ui: &MainWindow,
    project_name: &str,
    project_key: &str,
    cwd: &str,
) {
    let Some(shared_state) = TERMINAL_SHARED_STATE.get() else {
        return;
    };

    let preserve_state = shared_state
        .lock()
        .map(|guard| guard.project_key == project_key && guard.session.is_some())
        .unwrap_or(false);

    if let Ok(mut guard) = shared_state.lock() {
        guard.project_name = project_name.trim().to_string();
        guard.drawer_visible = true;
        guard.dirty = true;
    }

    let title = terminal_title_for_project(project_name);
    log::info!(
        "[Terminal] Open requested from page context: route='{}', source='project-card', title='{}', project_key='{}', cwd='{}', preserve_state={}",
        current_route_label(ui),
        title,
        project_key,
        cwd,
        preserve_state
    );
    ui.set_shell_drawer_title(title.clone().into());

    if !preserve_state {
        reset_terminal_for_project(ui, project_key, cwd, false);
        ui.set_shell_drawer_title(title.into());
    }

    ui.set_shell_drawer_visible(true);
    ensure_session_started(ui, shared_state.clone());
}

pub(crate) fn terminal_title_for_project(project_name: &str) -> String {
    if project_name.trim().is_empty() {
        crate::i18n::tr("terminal.project_title_default").into_owned()
    } else {
        crate::i18n::tr_args(
            "terminal.project_title",
            &[("project", project_name.trim())],
        )
    }
}

fn bind_terminal(ui: &MainWindow, shared_state: SharedTerminalState) {
    let ui_weak = ui.as_weak();
    let state_toggle = shared_state.clone();
    ui.on_shell_drawer_toggle(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        if ui.get_shell_drawer_visible() {
            log::info!(
                "[Terminal] Toggle close requested: route='{}', source='{}', title='{}', cwd='{}', fullscreen={}",
                current_route_label(&ui),
                toggle_source_label(&ui),
                ui.get_shell_drawer_title(),
                ui.get_shell_drawer_cwd(),
                ui.get_shell_drawer_fullscreen()
            );
            set_drawer_visible(&state_toggle, false);
            ui.set_shell_drawer_visible(false);
            ui.set_shell_drawer_fullscreen(false);
            return;
        }

        log::info!(
            "[Terminal] Toggle open requested: route='{}', source='{}', title='{}', cwd='{}'",
            current_route_label(&ui),
            toggle_source_label(&ui),
            ui.get_shell_drawer_title(),
            ui.get_shell_drawer_cwd(),
        );
        set_drawer_visible(&state_toggle, true);
        ui.set_shell_drawer_visible(true);
        ensure_session_started(&ui, state_toggle.clone());
    });

    let ui_weak = ui.as_weak();
    let state_close = shared_state.clone();
    ui.on_shell_drawer_close(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        log::info!(
            "[Terminal] Close requested: route='{}', source='terminal-close-button', title='{}', cwd='{}', fullscreen={}",
            current_route_label(&ui),
            ui.get_shell_drawer_title(),
            ui.get_shell_drawer_cwd(),
            ui.get_shell_drawer_fullscreen()
        );
        set_drawer_visible(&state_close, false);
    });

    let state_submit = shared_state.clone();
    ui.on_shell_drawer_submit(move |command| {
        let command = command.trim().to_string();
        if command.is_empty() {
            return;
        }
        let mut bytes = command.into_bytes();
        bytes.push(b'\n');
        send_input_bytes(state_submit.clone(), &bytes);
    });

    let state_keys = shared_state.clone();
    ui.on_shell_drawer_key_pressed(move |text, ctrl, alt, shift, meta| {
        if terminal_verbose_logging_enabled() {
            log::info!(
                "[Terminal][Input] Key received: text='{}', ctrl={}, alt={}, shift={}, meta={}",
                sanitize_for_log(text.as_str()),
                ctrl,
                alt,
                shift,
                meta
            );
        }
        if let Some(bytes) = map_key_to_bytes(text.as_str(), ctrl, alt, shift, meta) {
            if terminal_verbose_logging_enabled() {
                log::info!(
                    "[Terminal][Input] Key mapped to bytes: len={}, preview={}",
                    bytes.len(),
                    format_bytes_for_log(&bytes)
                );
            }
            send_input_bytes(state_keys.clone(), &bytes);
        } else if terminal_verbose_logging_enabled() {
            log::warn!(
                "[Terminal][Input] Key produced no bytes: text='{}'",
                sanitize_for_log(text.as_str())
            );
        }
    });

    let ui_weak = ui.as_weak();
    let state_resize = shared_state.clone();
    ui.on_shell_drawer_resize(move |cols, rows| {
        let cols = cols.max(1) as u16;
        let rows = rows.max(1) as u16;
        let Ok(mut guard) = state_resize.lock() else {
            return;
        };
        if !guard.drawer_visible {
            return;
        }
        if guard.cols != cols || guard.rows != rows {
            guard.cols = cols;
            guard.rows = rows;
            if let Some(session) = guard.session.as_mut() {
                let _ = session.resize(cols, rows);
            }
            guard.render.resize(rows, cols);
            guard.max_scrollback_dirty = true;
            guard.output_dirty = guard.selection_mode || !ENABLE_TERMINAL_SPAN_RENDER;
            refresh_render_snapshot(&mut guard);
            guard.dirty = true;
            log_terminal_diagnostics("resize", &mut guard, None, true);
        }
        let should_start_session = guard.session.is_none();
        drop(guard);

        schedule_refresh(ui_weak.clone(), state_resize.clone());

        if should_start_session {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ensure_session_started(&ui, state_resize.clone());
        }
    });

    let state_scrollback = shared_state.clone();
    let ui_weak_scrollback = ui.as_weak();
    ui.on_shell_drawer_scrollback_requested(move |delta| {
        let Ok(mut guard) = state_scrollback.lock() else {
            return;
        };
        let current = guard.render.scrollback() as i32;
        let max = guard.max_scrollback as i32;
        let next = (current + delta).clamp(0, max) as usize;
        if next == current as usize {
            return;
        }
        guard.render.set_scrollback(next);
        guard.dirty = true;
        log_terminal_diagnostics("scroll", &mut guard, None, false);
        drop(guard);
        schedule_refresh(ui_weak_scrollback.clone(), state_scrollback.clone());
    });

    let state_paste = shared_state.clone();
    ui.on_shell_drawer_paste(move || match clipboard_get_text() {
        Ok(text) => {
            if text.is_empty() {
                return;
            }
            if terminal_verbose_logging_enabled() {
                log::info!(
                    "[Terminal][Clipboard] Paste requested: chars={}",
                    text.chars().count()
                );
            }
            send_input_bytes(state_paste.clone(), text.as_bytes());
        }
        Err(error) => {
            log::error!("[Terminal][Clipboard] Paste read failed: {}", error);
        }
    });

    let state_selection_mode = shared_state.clone();
    let ui_weak_selection_mode = ui.as_weak();
    ui.on_shell_drawer_selection_mode_changed(move |enabled| {
        let Ok(mut guard) = state_selection_mode.lock() else {
            return;
        };
        guard.selection_mode = enabled;
        if enabled {
            guard.output_dirty = !guard.lines.is_empty() || guard.selection_output.is_empty();
        } else if ENABLE_TERMINAL_SPAN_RENDER && !guard.lines.is_empty() {
            clear_selection_output(&mut guard);
        }
        guard.dirty = true;
        drop(guard);
        schedule_refresh(ui_weak_selection_mode.clone(), state_selection_mode.clone());
    });
}

fn stop_session_locked(guard: &mut TerminalDrawerState, append_closed_notice: bool) {
    if !append_closed_notice {
        log_terminal_diagnostics("close", guard, None, true);
        guard.rows = DEFAULT_ROWS;
        guard.cols = DEFAULT_COLS;
        terminate_session_locked(guard);
        reset_terminal_view_state(guard);
        return;
    }

    if guard.session.is_none() {
        guard.connected = false;
        guard.running = false;
        guard.dirty = true;
        return;
    }

    terminate_session_locked(guard);
    guard.connected = false;
    guard.running = false;
    if append_closed_notice {
        let prefix = selection_output_without_notice(guard);
        set_selection_notice(guard, prefix, "terminal.session_closed");
    }
    guard.selection_mode = false;
    guard.output_dirty = false;
    guard.dirty = true;
}

fn initialize_ui(ui: &MainWindow, shared_state: &SharedTerminalState) {
    let Ok(guard) = shared_state.lock() else {
        return;
    };
    ui.set_shell_drawer_cwd(guard.cwd.clone().into());
    ui.set_shell_drawer_output(guard.selection_output.clone().into());
    ui.set_shell_drawer_output_line_count(guard.selection_output_line_count);
    if ENABLE_TERMINAL_SPAN_RENDER {
        let lines_model = sync_ui_line_models(&guard.lines);
        ui.set_shell_drawer_lines(lines_model);
    } else {
        let lines_model = sync_ui_line_models(&[]);
        ui.set_shell_drawer_lines(lines_model);
    }
    ui.set_shell_drawer_scrollback_offset(guard.scrollback_offset as i32);
    ui.set_shell_drawer_max_scrollback(guard.max_scrollback as i32);
    ui.set_shell_drawer_cursor_row(guard.cursor.row as i32);
    ui.set_shell_drawer_cursor_col(guard.cursor.col as i32);
    ui.set_shell_drawer_cursor_visible(guard.cursor.visible);
    ui.set_shell_drawer_running(guard.running);
    ui.set_shell_drawer_connected(guard.connected);
    ui.set_shell_drawer_input_failed(guard.input_failed);
}

fn ensure_session_started(ui: &MainWindow, shared_state: SharedTerminalState) {
    let cwd = ui.get_shell_drawer_cwd().to_string();
    let title = ui.get_shell_drawer_title().to_string();
    let ui_weak = ui.as_weak();

    let tx = {
        let Ok(mut guard) = shared_state.lock() else {
            return;
        };
        if guard.retirement.closing {
            return;
        }
        guard.drawer_visible = true;
        if guard.session.is_some() {
            guard.connected = true;
            guard.dirty = true;
            drop(guard);
            schedule_refresh(ui_weak, shared_state.clone());
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded::<SessionEvent>(session::OUTPUT_CHUNKS);
        match PtySession::spawn(&cwd, guard.cols, guard.rows, tx.clone()) {
            Ok(session) => {
                let session_id = guard.next_session_id;
                guard.next_session_id = guard.next_session_id.saturating_add(1);
                log::info!(
                    "[Terminal] PTY session started: route='{}', title='{}', cwd='{}', rows={}, cols={}",
                    current_route_label(ui),
                    title,
                    cwd,
                    guard.rows,
                    guard.cols
                );
                guard.session = Some(session);
                guard.active_session_id = Some(session_id);
                guard.connected = true;
                guard.input_failed = false;
                guard.running = false;
                guard.dirty = true;
                log_terminal_diagnostics("session_started", &mut guard, None, true);
                spawn_session_event_bridge(shared_state.clone(), rx, ui_weak.clone(), session_id);
                tx
            }
            Err(error) => {
                log::error!("[Terminal] Failed to start PTY session: {}", error);
                set_selection_notice(&mut guard, String::new(), "terminal.start_failed");
                guard.output_dirty = false;
                guard.connected = false;
                guard.running = false;
                guard.dirty = true;
                drop(guard);
                schedule_refresh(ui_weak, shared_state.clone());
                return;
            }
        }
    };

    drop(tx);
    schedule_refresh(ui_weak, shared_state.clone());
}

fn spawn_session_event_bridge(
    shared_state: SharedTerminalState,
    rx: crossbeam_channel::Receiver<SessionEvent>,
    ui_weak: slint::Weak<MainWindow>,
    session_id: u64,
) {
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let Ok(mut guard) = shared_state.lock() else {
                break;
            };
            if guard.active_session_id != Some(session_id) {
                if matches!(event, SessionEvent::Exit) {
                    break;
                }
                continue;
            }
            let mut should_exit = false;
            match event {
                SessionEvent::Output(bytes) => {
                    log_terminal_output_chunk(&mut guard, &bytes);
                    if TerminalRenderState::contains_clear_scrollback_escape(&bytes) {
                        log::info!(
                            "[Terminal][Output] Clear scrollback escape detected; resetting terminal buffer"
                        );
                        guard.render.clear_buffer();
                    }
                    let replies = guard.render.process_with_replies(&bytes);
                    // Keep the live prompt visible after each PTY update.
                    guard.render.set_scrollback(0);
                    guard.max_scrollback_dirty = true;
                    guard.output_dirty = guard.selection_mode || !ENABLE_TERMINAL_SPAN_RENDER;
                    for reply in replies {
                        if let Some(session) = guard.session.as_mut() {
                            match session.enqueue_input(&reply) {
                                Ok(()) => {
                                    if terminal_verbose_logging_enabled() {
                                        log::info!(
                                            "[Terminal][Reply] Queued terminal reply: len={}, preview={}",
                                            reply.len(),
                                            format_bytes_for_log(&reply)
                                        );
                                    }
                                }
                                Err(error) => {
                                    log::error!(
                                        "[Terminal][Reply] Failed to send terminal reply: {}",
                                        error
                                    );
                                    stop_session_locked(&mut guard, true);
                                    guard.input_failed = true;
                                    should_exit = true;
                                    break;
                                }
                            }
                        }
                    }
                    guard.dirty = true;
                }
                SessionEvent::WriteFailed => {
                    stop_session_locked(&mut guard, true);
                    guard.input_failed = true;
                    guard.dirty = true;
                    should_exit = true;
                }
                SessionEvent::Exit => {
                    log::info!("[Terminal][Lifecycle] PTY session exit received");
                    if guard.session.is_none() {
                        should_exit = true;
                    } else {
                        stop_session_locked(&mut guard, true);
                        should_exit = true;
                    }
                }
            }
            drop(guard);
            schedule_refresh(ui_weak.clone(), shared_state.clone());
            if should_exit {
                break;
            }
        }
    });
}

fn refresh_render_snapshot(guard: &mut TerminalDrawerState) {
    let refresh_started = terminal_verbose_logging_enabled().then(Instant::now);
    if ENABLE_TERMINAL_SPAN_RENDER {
        guard.render.rendered_lines_into(&mut guard.lines);
    } else {
        guard.lines.clear();
    }
    guard.scrollback_offset = guard.render.scrollback();
    if guard.max_scrollback_dirty {
        guard.max_scrollback = guard.render.max_scrollback();
        guard.max_scrollback_dirty = false;
    }
    guard.cursor = guard.render.cursor_state();

    let needs_text_output =
        !ENABLE_TERMINAL_SPAN_RENDER || guard.selection_mode || guard.lines.is_empty();
    if needs_text_output && guard.output_dirty {
        let output = guard.render.rendered_text();
        set_selection_output_text(guard, output);
        guard.output_dirty = false;
    } else if !needs_text_output && !guard.selection_output.is_empty() {
        clear_selection_output(guard);
    }
    let refresh_duration_ms = refresh_started.map(|started| started.elapsed().as_millis() as u64);
    log_terminal_diagnostics("refresh", guard, refresh_duration_ms, false);
}

fn default_terminal_lines() -> Vec<RenderedTerminalLine> {
    Vec::new()
}

fn send_input_bytes(shared_state: SharedTerminalState, bytes: &[u8]) {
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    let mut schedule_refresh_after_write = false;
    if guard.render.scrollback() > 0 {
        guard.render.set_scrollback(0);
        guard.scrollback_offset = 0;
        guard.dirty = true;
        schedule_refresh_after_write = true;
    }
    if let Some(session) = guard.session.as_mut() {
        match session.enqueue_input(bytes) {
            Ok(()) => {
                if guard.input_failed {
                    guard.input_failed = false;
                    guard.dirty = true;
                    schedule_refresh_after_write = true;
                }
                if terminal_verbose_logging_enabled() {
                    log::info!(
                        "[Terminal][Input] PTY input queued: len={}, preview={}",
                        bytes.len(),
                        format_bytes_for_log(bytes)
                    );
                }
            }
            Err(error) => {
                log::error!("[Terminal] Failed to write PTY input: {}", error);
                guard.input_failed = true;
                let prefix = selection_output_without_notice(&guard);
                set_selection_notice(&mut guard, prefix, "terminal.write_failed");
                guard.output_dirty = false;
                guard.dirty = true;
                drop(guard);
                schedule_global_refresh(shared_state.clone());
                return;
            }
        }
    } else {
        if terminal_verbose_logging_enabled() {
            log::warn!(
                "[Terminal][Input] PTY write skipped because session is not attached: len={}, preview={}",
                bytes.len(),
                format_bytes_for_log(bytes)
            );
        }
    }
    if schedule_refresh_after_write {
        drop(guard);
        schedule_global_refresh(shared_state.clone());
    }
}

fn schedule_global_refresh(shared_state: SharedTerminalState) {
    let Some(ui_weak) = TERMINAL_UI_WEAK.get().cloned() else {
        return;
    };
    schedule_refresh(ui_weak, shared_state);
}

fn sync_ui_line_models(lines: &[RenderedTerminalLine]) -> ModelRc<TerminalLine> {
    TERMINAL_UI_MODELS.with(|cell| {
        let mut state = cell.borrow_mut();
        let models = state.get_or_insert_with(|| TerminalUiModels {
            lines_model: Rc::new(VecModel::from(Vec::<TerminalLine>::new())),
            span_models: Vec::new(),
            cached_lines: Vec::new(),
        });

        while models.span_models.len() > lines.len() {
            let last = models.span_models.len() - 1;
            models.lines_model.remove(last);
            models.span_models.pop();
            models.cached_lines.pop();
        }

        while models.span_models.len() < lines.len() {
            let span_model = Rc::new(VecModel::from(Vec::<TerminalSpan>::new()));
            models.lines_model.push(TerminalLine {
                spans: ModelRc::new(span_model.clone()),
            });
            models.span_models.push(span_model);
            models
                .cached_lines
                .push(RenderedTerminalLine { spans: Vec::new() });
        }

        for (index, line) in lines.iter().enumerate() {
            if models.cached_lines[index] != *line {
                models.span_models[index].set_vec(render::to_ui_spans(&line.spans));
                models.cached_lines[index] = line.clone();
            }
        }

        ModelRc::new(models.lines_model.clone())
    })
}

fn schedule_refresh(_ui_weak: slint::Weak<MainWindow>, shared_state: SharedTerminalState) {
    let maybe_delay = {
        let Ok(mut guard) = shared_state.lock() else {
            return;
        };
        begin_refresh_schedule(&mut guard, Instant::now())
    };

    let Some(delay) = maybe_delay else {
        return;
    };

    let shared_state_for_ui = shared_state.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let run_refresh = move || {
            let Some(ui_weak) = TERMINAL_UI_WEAK.get().cloned() else {
                if let Ok(mut guard) = shared_state_for_ui.lock() {
                    guard.refresh_scheduled = false;
                }
                return;
            };
            let Some(ui) = ui_weak.upgrade() else {
                if let Ok(mut guard) = shared_state_for_ui.lock() {
                    guard.refresh_scheduled = false;
                }
                return;
            };

            let should_apply = if let Ok(mut guard) = shared_state_for_ui.lock() {
                if !guard.drawer_visible {
                    guard.refresh_scheduled = false;
                    false
                } else {
                    if guard.dirty {
                        refresh_render_snapshot(&mut guard);
                        guard.dirty = false;
                    }
                    guard.refresh_scheduled = false;
                    guard.last_refresh_applied_at = Some(Instant::now());
                    true
                }
            } else {
                false
            };
            if should_apply {
                initialize_ui(&ui, &shared_state_for_ui);
            }
        };

        if delay.is_zero() {
            run_refresh();
        } else {
            Timer::single_shot(delay, run_refresh);
        }
    });
}

fn begin_refresh_schedule(guard: &mut TerminalDrawerState, now: Instant) -> Option<Duration> {
    if !guard.drawer_visible || guard.refresh_scheduled {
        return None;
    }
    guard.refresh_scheduled = true;
    Some(next_refresh_delay(
        now,
        guard.last_refresh_applied_at,
        TERMINAL_REFRESH_FRAME_INTERVAL,
    ))
}

fn terminate_session_locked(guard: &mut TerminalDrawerState) {
    if let Some(session) = detach_session_locked(guard) {
        guard.retirement.retire(session, "session-exit");
    }
}

fn detach_session_locked(guard: &mut TerminalDrawerState) -> Option<PtySession> {
    let session = guard.session.take();
    guard.active_session_id = None;
    guard.connected = false;
    guard.input_failed = false;
    guard.running = false;
    guard.render.set_scrollback(0);
    guard.scrollback_offset = 0;
    guard.max_scrollback_dirty = false;
    session
}

fn set_drawer_visible(shared_state: &SharedTerminalState, visible: bool) {
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    guard.drawer_visible = visible;
    if visible {
        guard.dirty = true;
    }
}

fn reset_terminal_view_state(guard: &mut TerminalDrawerState) {
    guard.render = TerminalRenderState::new(guard.rows, guard.cols, default_scrollback());
    guard.selection_mode = false;
    clear_selection_output(guard);
    guard.output_dirty = false;
    guard.lines = default_terminal_lines();
    guard.scrollback_offset = 0;
    guard.max_scrollback = 0;
    guard.max_scrollback_dirty = false;
    guard.cursor = TerminalCursorState::default();
    guard.running = false;
    guard.connected = false;
    guard.dirty = true;
    guard.last_diagnostics_log = None;
    guard.last_pty_output_log = None;
    guard.suppressed_pty_output_chunks = 0;
    guard.suppressed_pty_output_bytes = 0;
}

fn log_terminal_diagnostics(
    reason: &str,
    guard: &mut TerminalDrawerState,
    refresh_duration_ms: Option<u64>,
    force: bool,
) {
    if !terminal_verbose_logging_enabled() {
        return;
    }

    if !force {
        let now = Instant::now();
        if let Some(last) = guard.last_diagnostics_log
            && now.duration_since(last) < Duration::from_millis(500)
        {
            return;
        }
        guard.last_diagnostics_log = Some(now);
    } else {
        guard.last_diagnostics_log = Some(Instant::now());
    }

    let (span_count, span_text_bytes) = terminal_line_stats(&guard.lines);
    let memory = process_memory_snapshot();
    let scrollback_capacity = guard.render.scrollback_capacity();
    let buffer_cells_estimate =
        guard.cols as u64 * (guard.rows as u64 + scrollback_capacity as u64);
    let refresh_suffix = refresh_duration_ms
        .map(|ms| format!(", refresh_ms={ms}"))
        .unwrap_or_default();

    log::info!(
        "[Terminal][Diag] reason={}, rows={}, cols={}, lines={}, spans={}, span_text_bytes={}, selection_output_bytes={}, scrollback_offset={}, max_scrollback={}, scrollback_capacity={}, buffer_cells_estimate={}, selection_mode={}, connected={}, running={}, rss_current_bytes={}{}",
        reason,
        guard.rows,
        guard.cols,
        guard.lines.len(),
        span_count,
        span_text_bytes,
        guard.selection_output.len(),
        guard.scrollback_offset,
        guard.max_scrollback,
        scrollback_capacity,
        buffer_cells_estimate,
        guard.selection_mode,
        guard.connected,
        guard.running,
        memory.rss_current_bytes,
        refresh_suffix,
    );
}

fn log_terminal_output_chunk(guard: &mut TerminalDrawerState, bytes: &[u8]) {
    if !terminal_verbose_logging_enabled() {
        return;
    }

    let now = Instant::now();
    if let Some(last) = guard.last_pty_output_log
        && now.duration_since(last) < Duration::from_millis(500)
    {
        guard.suppressed_pty_output_chunks += 1;
        guard.suppressed_pty_output_bytes += bytes.len();
        return;
    }

    if guard.suppressed_pty_output_chunks > 0 {
        log::info!(
            "[Terminal][Output] Suppressed {} PTY chunks ({} bytes) in last interval",
            guard.suppressed_pty_output_chunks,
            guard.suppressed_pty_output_bytes
        );
        guard.suppressed_pty_output_chunks = 0;
        guard.suppressed_pty_output_bytes = 0;
    }

    guard.last_pty_output_log = Some(now);
    log::info!(
        "[Terminal][Output] PTY bytes received: len={}, preview={}",
        bytes.len(),
        format_bytes_for_log(bytes)
    );
}

fn clear_selection_output(guard: &mut TerminalDrawerState) {
    guard.selection_output.clear();
    guard.selection_output_line_count = 0;
    guard.selection_output_prefix.clear();
    guard.selection_notice_key = None;
}

fn set_selection_output_text(guard: &mut TerminalDrawerState, output: String) {
    guard.selection_output_line_count = count_output_lines(&output);
    guard.selection_output = output;
    guard.selection_output_prefix.clear();
    guard.selection_notice_key = None;
}

fn set_selection_notice(guard: &mut TerminalDrawerState, prefix: String, key: &'static str) {
    guard.selection_output_prefix = prefix;
    guard.selection_notice_key = Some(key);
    refresh_selection_notice(guard);
}

fn refresh_selection_notice(guard: &mut TerminalDrawerState) {
    let Some(key) = guard.selection_notice_key else {
        return;
    };
    let mut output = guard.selection_output_prefix.clone();
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(crate::i18n::tr(key).as_ref());
    output.push('\n');
    guard.selection_output_line_count = count_output_lines(&output);
    guard.selection_output = output;
}

fn selection_output_without_notice(guard: &TerminalDrawerState) -> String {
    if guard.selection_notice_key.is_some() {
        guard.selection_output_prefix.clone()
    } else {
        guard.selection_output.clone()
    }
}

fn terminal_notice_line(key: &'static str) -> String {
    let mut line = crate::i18n::tr(key).into_owned();
    line.push('\n');
    line
}

fn count_output_lines(output: &str) -> i32 {
    if output.is_empty() {
        0
    } else {
        output.lines().count() as i32
    }
}

fn default_terminal_cwd() -> String {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

fn clipboard_get_text() -> Result<String, String> {
    match Clipboard::new() {
        Ok(mut clipboard) => clipboard
            .get_text()
            .map_err(|error| format!("arboard get_text failed: {error}")),
        Err(error) => {
            #[cfg(target_os = "macos")]
            {
                let output = Command::new("pbpaste")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(|spawn_error| {
                        format!("arboard init failed: {error}; pbpaste spawn failed: {spawn_error}")
                    })?;
                if output.status.success() {
                    String::from_utf8(output.stdout)
                        .map_err(|utf_error| format!("pbpaste utf8 decode failed: {utf_error}"))
                } else {
                    Err(format!(
                        "arboard init failed: {error}; pbpaste exited with {}",
                        output.status
                    ))
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(format!("clipboard init failed: {error}"))
            }
        }
    }
}

fn current_route_label(ui: &MainWindow) -> &'static str {
    route_label(ui.get_current_route())
}

fn toggle_source_label(ui: &MainWindow) -> &'static str {
    match ui.get_current_route() {
        Route::Workflow => "workflow-fab",
        Route::Project => "project-terminal-toggle",
        _ => "shell-toggle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounted_input_failure_refreshes_with_runtime_locale() {
        use i_slint_backend_testing::ElementHandle;
        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.set_current_route(Route::Project);
        ui.set_shell_drawer_visible(true);
        ui.set_shell_drawer_input_failed(true);
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            let message = usher_i18n::translate_for(locale, "terminal.write_failed");
            assert!(
                ElementHandle::find_by_accessible_label(&ui, message.as_ref())
                    .next()
                    .is_some()
            );
        }
        crate::i18n::apply_preference(usher_i18n::source_locale().id).unwrap();
    }

    #[test]
    fn hidden_drawer_defers_refresh_until_reopened() {
        let now = Instant::now();
        let mut state = TerminalDrawerState::default();
        state.dirty = true;

        assert_eq!(begin_refresh_schedule(&mut state, now), None);
        assert!(!state.refresh_scheduled);
        assert!(state.dirty);

        state.drawer_visible = true;
        assert_eq!(
            begin_refresh_schedule(&mut state, now),
            Some(Duration::ZERO)
        );
        assert!(state.refresh_scheduled);
        assert!(state.dirty);
        assert_eq!(begin_refresh_schedule(&mut state, now), None);
    }

    #[test]
    fn rapid_visibility_toggles_keep_the_last_requested_state() {
        let shared_state = Arc::new(Mutex::new(TerminalDrawerState::default()));

        for _ in 0..100 {
            set_drawer_visible(&shared_state, true);
            set_drawer_visible(&shared_state, false);
        }
        set_drawer_visible(&shared_state, true);

        let state = shared_state.lock().expect("terminal state lock");
        assert!(state.drawer_visible);
        assert!(state.dirty);
        assert!(state.session.is_none());
    }
}
