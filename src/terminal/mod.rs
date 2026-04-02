mod render;
mod session;

use crate::{MainWindow, Route};
use arboard::Clipboard;
use render::{RenderedTerminalLine, TerminalCursorState, TerminalRenderState};
use session::{PtySession, SessionEvent};
use slint::{ComponentHandle, Timer, TimerMode};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex, OnceLock,
    mpsc::{self},
};
use std::time::Duration;

type SharedTerminalState = Arc<Mutex<TerminalDrawerState>>;
static TERMINAL_SHARED_STATE: OnceLock<SharedTerminalState> = OnceLock::new();

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_SCROLLBACK: usize = 10_000;
const ENABLE_TERMINAL_SPAN_RENDER: bool = true;

struct TerminalDrawerState {
    project_key: String,
    cwd: String,
    output: String,
    lines: Vec<RenderedTerminalLine>,
    scrollback_offset: usize,
    max_scrollback: usize,
    cursor: TerminalCursorState,
    running: bool,
    connected: bool,
    rows: u16,
    cols: u16,
    dirty: bool,
    render: TerminalRenderState,
    session: Option<PtySession>,
}

impl Default for TerminalDrawerState {
    fn default() -> Self {
        let render = TerminalRenderState::new(DEFAULT_ROWS, DEFAULT_COLS, DEFAULT_SCROLLBACK);
        Self {
            project_key: String::new(),
            cwd: default_terminal_cwd(),
            output: "Terminal ready.\n".to_string(),
            lines: default_terminal_lines(),
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: TerminalCursorState::default(),
            running: false,
            connected: false,
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            dirty: false,
            render,
            session: None,
        }
    }
}

pub fn setup_terminal(ui: &MainWindow) {
    let shared_state = Arc::new(Mutex::new(TerminalDrawerState::default()));
    let _ = TERMINAL_SHARED_STATE.set(shared_state.clone());
    initialize_ui(ui, &shared_state);
    bind_terminal(ui, shared_state.clone());
    start_ui_refresh_loop(ui, shared_state);
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
    guard.render = TerminalRenderState::new(guard.rows, guard.cols, DEFAULT_SCROLLBACK);
    guard.output = "Terminal ready.\n".to_string();
    guard.lines = default_terminal_lines();
    guard.scrollback_offset = 0;
    guard.max_scrollback = 0;
    guard.cursor = TerminalCursorState::default();
    guard.running = false;
    guard.connected = false;
    guard.dirty = true;
    drop(guard);

    ui.set_shell_drawer_visible(false);
    ui.set_shell_drawer_input_text("".into());
    ui.set_shell_drawer_height(0.0);
    ui.set_shell_drawer_fullscreen(false);
    initialize_ui(ui, shared_state);
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
        .map(|guard| guard.project_key == project_key)
        .unwrap_or(false);

    let title = if project_name.trim().is_empty() {
        "Project Terminal".to_string()
    } else {
        format!("{} Terminal", project_name.trim())
    };
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
            stop_session(state_toggle.clone(), false);
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
        stop_session(state_close.clone(), false);
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
        log::info!(
            "[Terminal][Input] Key received: text='{}', ctrl={}, alt={}, shift={}, meta={}",
            sanitize_for_log(text.as_str()),
            ctrl,
            alt,
            shift,
            meta
        );
        if let Some(bytes) = map_key_to_bytes(text.as_str(), ctrl, alt, shift, meta) {
            log::info!(
                "[Terminal][Input] Key mapped to bytes: len={}, preview={}",
                bytes.len(),
                format_bytes_for_log(&bytes)
            );
            send_input_bytes(state_keys.clone(), &bytes);
        } else {
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
        if guard.cols != cols || guard.rows != rows {
            guard.cols = cols;
            guard.rows = rows;
            if let Some(session) = guard.session.as_mut() {
                let _ = session.resize(cols, rows);
            }
            guard.render.resize(rows, cols);
            refresh_render_snapshot(&mut guard);
            guard.dirty = true;
        }
        let should_start_session = guard.session.is_none();
        drop(guard);

        if should_start_session {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ensure_session_started(&ui, state_resize.clone());
        }
    });

    let state_scrollback = shared_state.clone();
    ui.on_shell_drawer_scrollback_requested(move |delta| {
        let Ok(mut guard) = state_scrollback.lock() else {
            return;
        };
        let current = guard.render.scrollback() as i32;
        let max = guard.render.max_scrollback() as i32;
        let next = (current + delta).clamp(0, max) as usize;
        guard.render.set_scrollback(next);
        refresh_render_snapshot(&mut guard);
        guard.dirty = true;
    });

    let state_paste = shared_state.clone();
    ui.on_shell_drawer_paste(move || match clipboard_get_text() {
        Ok(text) => {
            if text.is_empty() {
                return;
            }
            log::info!(
                "[Terminal][Clipboard] Paste requested: chars={}",
                text.chars().count()
            );
            send_input_bytes(state_paste.clone(), text.as_bytes());
        }
        Err(error) => {
            log::error!("[Terminal][Clipboard] Paste read failed: {}", error);
        }
    });
}

fn stop_session(shared_state: SharedTerminalState, append_closed_notice: bool) {
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    stop_session_locked(&mut guard, append_closed_notice);
}

fn stop_session_locked(guard: &mut TerminalDrawerState, append_closed_notice: bool) {
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
        if !guard.output.ends_with('\n') {
            guard.output.push('\n');
        }
        guard.output.push_str("[terminal session closed]\n");
    }
    guard.dirty = true;
}

fn start_ui_refresh_loop(ui: &MainWindow, shared_state: SharedTerminalState) {
    let ui_weak = ui.as_weak();
    let timer = Box::leak(Box::new(Timer::default()));
    timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let should_refresh = shared_state
            .lock()
            .map(|guard| guard.dirty)
            .unwrap_or(false);
        if !should_refresh {
            return;
        }
        if let Ok(mut guard) = shared_state.lock() {
            refresh_render_snapshot(&mut guard);
            guard.dirty = false;
        }
        initialize_ui(&ui, &shared_state);
    });
}

fn initialize_ui(ui: &MainWindow, shared_state: &SharedTerminalState) {
    let Ok(guard) = shared_state.lock() else {
        return;
    };
    ui.set_shell_drawer_cwd(guard.cwd.clone().into());
    ui.set_shell_drawer_output(guard.output.clone().into());
    ui.set_shell_drawer_output_line_count(guard.output.lines().count() as i32);
    if ENABLE_TERMINAL_SPAN_RENDER {
        ui.set_shell_drawer_lines(slint::ModelRc::new(slint::VecModel::from(
            render::to_ui_lines(&guard.lines),
        )));
    } else {
        ui.set_shell_drawer_lines(slint::ModelRc::new(slint::VecModel::from(Vec::new())));
    }
    ui.set_shell_drawer_scrollback_offset(guard.scrollback_offset as i32);
    ui.set_shell_drawer_max_scrollback(guard.max_scrollback as i32);
    ui.set_shell_drawer_cursor_row(guard.cursor.row as i32);
    ui.set_shell_drawer_cursor_col(guard.cursor.col as i32);
    ui.set_shell_drawer_cursor_visible(guard.cursor.visible);
    ui.set_shell_drawer_running(guard.running);
    ui.set_shell_drawer_connected(guard.connected);
}

fn ensure_session_started(ui: &MainWindow, shared_state: SharedTerminalState) {
    let cwd = ui.get_shell_drawer_cwd().to_string();
    let title = ui.get_shell_drawer_title().to_string();

    let tx = {
        let Ok(mut guard) = shared_state.lock() else {
            return;
        };
        if guard.session.is_some() {
            guard.connected = true;
            guard.dirty = true;
            return;
        }
        let (tx, rx) = mpsc::channel::<SessionEvent>();
        match PtySession::spawn(&cwd, guard.cols, guard.rows, tx.clone()) {
            Ok(session) => {
                log::info!(
                    "[Terminal] PTY session started: route='{}', title='{}', cwd='{}', rows={}, cols={}",
                    current_route_label(ui),
                    title,
                    cwd,
                    guard.rows,
                    guard.cols
                );
                guard.session = Some(session);
                guard.connected = true;
                guard.running = false;
                guard.dirty = true;
            }
            Err(error) => {
                log::error!("[Terminal] Failed to start PTY session: {}", error);
                guard.output = format!("Failed to start terminal session:\n{}\n", error);
                guard.connected = false;
                guard.running = false;
                guard.dirty = true;
                return;
            }
        }
        spawn_session_event_bridge(shared_state.clone(), rx);
        tx
    };

    drop(tx);
}

fn spawn_session_event_bridge(
    shared_state: SharedTerminalState,
    rx: std::sync::mpsc::Receiver<SessionEvent>,
) {
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let Ok(mut guard) = shared_state.lock() else {
                break;
            };
            match event {
                SessionEvent::Output(bytes) => {
                    log::info!(
                        "[Terminal][Output] PTY bytes received: len={}, preview={}",
                        bytes.len(),
                        format_bytes_for_log(&bytes)
                    );
                    if TerminalRenderState::contains_clear_scrollback_escape(&bytes) {
                        log::info!(
                            "[Terminal][Output] Clear scrollback escape detected; resetting terminal buffer"
                        );
                        guard.render.clear_buffer();
                    }
                    let replies = guard.render.process_with_replies(&bytes);
                    // Keep the live prompt visible after each PTY update.
                    guard.render.set_scrollback(0);
                    for reply in replies {
                        if let Some(session) = guard.session.as_mut() {
                            match session.write_bytes(&reply) {
                                Ok(()) => {
                                    log::info!(
                                        "[Terminal][Reply] Sent terminal reply: len={}, preview={}",
                                        reply.len(),
                                        format_bytes_for_log(&reply)
                                    );
                                }
                                Err(error) => {
                                    log::error!(
                                        "[Terminal][Reply] Failed to send terminal reply: {}",
                                        error
                                    );
                                }
                            }
                        }
                    }
                    guard.dirty = true;
                }
                SessionEvent::Exit => {
                    log::info!("[Terminal][Lifecycle] PTY session exit received");
                    stop_session_locked(&mut guard, true);
                    break;
                }
            }
        }
    });
}

fn refresh_render_snapshot(guard: &mut TerminalDrawerState) {
    guard.output = guard.render.rendered_text();
    if ENABLE_TERMINAL_SPAN_RENDER {
        guard.lines = guard.render.rendered_lines();
    } else {
        guard.lines.clear();
    }
    guard.scrollback_offset = guard.render.scrollback();
    guard.max_scrollback = guard.render.max_scrollback();
    guard.cursor = guard.render.cursor_state();
}

fn default_terminal_lines() -> Vec<RenderedTerminalLine> {
    Vec::new()
}

fn send_input_bytes(shared_state: SharedTerminalState, bytes: &[u8]) {
    let Ok(mut guard) = shared_state.lock() else {
        return;
    };
    if guard.render.scrollback() > 0 {
        guard.render.set_scrollback(0);
        refresh_render_snapshot(&mut guard);
    }
    if let Some(session) = guard.session.as_mut() {
        match session.write_bytes(bytes) {
            Ok(()) => {
                log::info!(
                    "[Terminal][Input] PTY write succeeded: len={}, preview={}",
                    bytes.len(),
                    format_bytes_for_log(bytes)
                );
            }
            Err(error) => {
                log::error!("[Terminal] Failed to write PTY input: {}", error);
                if !guard.output.ends_with('\n') {
                    guard.output.push('\n');
                }
                guard
                    .output
                    .push_str(&format!("[terminal write failed] {}\n", error));
                guard.dirty = true;
            }
        }
    } else {
        log::warn!(
            "[Terminal][Input] PTY write skipped because session is not attached: len={}, preview={}",
            bytes.len(),
            format_bytes_for_log(bytes)
        );
    }
}

fn terminate_session_locked(guard: &mut TerminalDrawerState) {
    if let Some(session) = guard.session.as_mut() {
        session.terminate();
    }
    guard.session = None;
    guard.connected = false;
    guard.running = false;
    guard.render.set_scrollback(0);
    guard.scrollback_offset = 0;
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

fn map_key_to_bytes(text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<Vec<u8>> {
    if text.is_empty() && !ctrl {
        return None;
    }

    let bytes = match text {
        "__IGNORE__" => return None,
        "__CTRL_C__" => vec![0x03],
        "__CTRL_D__" => vec![0x04],
        "__CTRL_L__" => vec![0x0c],
        "__CTRL_Z__" => vec![0x1a],
        "__RETURN__" => b"\r".to_vec(),
        "__BACKSPACE__" => vec![0x7f],
        "__DELETE__" => b"\x1b[3~".to_vec(),
        "__TAB__" => b"\t".to_vec(),
        "__ESCAPE__" => b"\x1b".to_vec(),
        "__LEFT__" => b"\x1b[D".to_vec(),
        "__RIGHT__" => b"\x1b[C".to_vec(),
        "__UP__" => b"\x1b[A".to_vec(),
        "__DOWN__" => b"\x1b[B".to_vec(),
        "__HOME__" => b"\x1b[H".to_vec(),
        "__END__" => b"\x1b[F".to_vec(),
        _ => {
            if ctrl && text.is_empty() {
                return None;
            }
            if ctrl && text.chars().count() == 1 {
                let ch = text.chars().next().unwrap_or_default();
                if ch.is_ascii_control() {
                    vec![ch as u8]
                } else {
                    let upper = ch.to_ascii_uppercase();
                    if upper.is_ascii_alphabetic() {
                        vec![(upper as u8) - b'@']
                    } else {
                        text.as_bytes().to_vec()
                    }
                }
            } else if alt || meta {
                let mut seq = vec![0x1b];
                seq.extend_from_slice(text.as_bytes());
                seq
            } else if shift {
                text.as_bytes().to_vec()
            } else {
                text.as_bytes().to_vec()
            }
        }
    };

    Some(bytes)
}

fn sanitize_for_log(text: &str) -> String {
    text.chars()
        .flat_map(|ch| ch.escape_default())
        .collect::<String>()
}

fn format_bytes_for_log(bytes: &[u8]) -> String {
    const MAX_PREVIEW: usize = 24;
    let mut parts = bytes
        .iter()
        .take(MAX_PREVIEW)
        .map(|byte| format!("{byte:#04x}"))
        .collect::<Vec<_>>();
    if bytes.len() > MAX_PREVIEW {
        parts.push("...".to_string());
    }
    format!("[{}]", parts.join(", "))
}

fn current_route_label(ui: &MainWindow) -> &'static str {
    match ui.get_current_route() {
        Route::Login => "Login",
        Route::Home => "Home",
        Route::Search => "Search",
        Route::Settings => "Settings",
        Route::Notification => "Notification",
        Route::Toast => "Toast",
        Route::Dialog => "Dialog",
        Route::Tools => "Tools",
        Route::Project => "Project",
        Route::Chat => "Chat",
        Route::About => "About",
        Route::List => "List",
        Route::Detail => "Detail",
        Route::Workflow => "Workflow",
    }
}

fn toggle_source_label(ui: &MainWindow) -> &'static str {
    match ui.get_current_route() {
        Route::Workflow => "workflow-fab",
        Route::Project => "project-terminal-toggle",
        _ => "shell-toggle",
    }
}
