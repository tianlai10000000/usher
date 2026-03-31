use crate::app_runtime::shared_runtime;
use crate::{MainWindow, Route};
use slint::{ComponentHandle, Model};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

type SharedTerminalState = Arc<Mutex<TerminalDrawerState>>;
static TERMINAL_SHARED_STATE: OnceLock<SharedTerminalState> = OnceLock::new();

#[derive(Debug, Clone)]
struct TerminalDrawerState {
    project_key: String,
    cwd: String,
    output: String,
    running: bool,
}

impl Default for TerminalDrawerState {
    fn default() -> Self {
        Self {
            project_key: String::new(),
            cwd: default_terminal_cwd(),
            output: "Terminal ready.\n".to_string(),
            running: false,
        }
    }
}

pub fn setup_terminal(ui: &MainWindow) {
    let shared_state = Arc::new(Mutex::new(TerminalDrawerState::default()));
    let _ = TERMINAL_SHARED_STATE.set(shared_state.clone());
    initialize_ui(ui, &shared_state);
    bind_terminal(ui, shared_state);
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

    guard.project_key = project_key.to_string();
    guard.cwd = if cwd.trim().is_empty() {
        default_terminal_cwd()
    } else {
        cwd.trim().to_string()
    };
    guard.output = "Terminal ready.\n".to_string();
    guard.running = false;
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

    if preserve_state {
        initialize_ui(ui, shared_state);
        ui.set_shell_drawer_visible(true);
        return;
    }

    reset_terminal_for_project(ui, project_key, cwd, false);
    ui.set_shell_drawer_title(title.into());
    ui.set_shell_drawer_visible(true);
    let Some(ui_handle) = ui.as_weak().upgrade() else {
        return;
    };
    run_command_sequence(
        ui_handle,
        shared_state.clone(),
        vec![format!("cd {}", cwd.trim())],
    );
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
            ui.set_shell_drawer_visible(false);
            return;
        }

        log::info!(
            "[Terminal] Toggle open requested: route='{}', source='{}', title='{}', cwd='{}', init_commands={}",
            current_route_label(&ui),
            toggle_source_label(&ui),
            ui.get_shell_drawer_title(),
            ui.get_shell_drawer_cwd(),
            collect_init_commands(&ui).len()
        );
        ui.set_shell_drawer_visible(true);
        let init_commands = collect_init_commands(&ui);
        run_command_sequence(ui, state_toggle.clone(), init_commands);
    });

    let ui_weak = ui.as_weak();
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
    });

    let ui_weak = ui.as_weak();
    let state_submit = shared_state.clone();
    ui.on_shell_drawer_submit(move |command| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let command = command.trim().to_string();
        if command.is_empty() {
            return;
        }
        run_command_sequence(ui, state_submit.clone(), vec![command]);
    });
}

fn collect_init_commands(ui: &MainWindow) -> Vec<String> {
    let model = ui.get_shell_drawer_init_commands();
    (0..model.row_count())
        .filter_map(|index| model.row_data(index))
        .map(|item| item.to_string())
        .filter(|item| !item.trim().is_empty())
        .collect()
}

fn initialize_ui(ui: &MainWindow, shared_state: &SharedTerminalState) {
    let Ok(guard) = shared_state.lock() else {
        return;
    };
    ui.set_shell_drawer_cwd(guard.cwd.clone().into());
    ui.set_shell_drawer_output(guard.output.clone().into());
    ui.set_shell_drawer_running(guard.running);
}

fn run_command_sequence(ui: MainWindow, shared_state: SharedTerminalState, commands: Vec<String>) {
    if commands.is_empty() {
        initialize_ui(&ui, &shared_state);
        return;
    }

    let cwd = {
        let Ok(mut guard) = shared_state.lock() else {
            return;
        };
        guard.running = true;
        if guard.cwd.trim().is_empty() {
            guard.cwd = default_terminal_cwd();
        }
        guard.cwd.clone()
    };
    initialize_ui(&ui, &shared_state);

    let runtime = shared_runtime();
    let ui_weak = ui.as_weak();
    runtime.spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || execute_command_sequence(cwd, commands)).await;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let Ok(mut guard) = shared_state.lock() else {
                return;
            };
            guard.running = false;
            match result {
                Ok(sequence_result) => {
                    guard.cwd = sequence_result.cwd;
                    for chunk in sequence_result.logs {
                        append_output(&mut guard.output, &chunk);
                    }
                }
                Err(error) => {
                    append_output(
                        &mut guard.output,
                        &format!("Command sequence join failed: {}", error),
                    );
                }
            }
            drop(guard);
            initialize_ui(&ui, &shared_state);
        });
    });
}

struct CommandSequenceResult {
    cwd: String,
    logs: Vec<String>,
}

fn execute_command_sequence(initial_cwd: String, commands: Vec<String>) -> CommandSequenceResult {
    let mut cwd = if initial_cwd.trim().is_empty() {
        default_terminal_cwd()
    } else {
        initial_cwd
    };
    let mut logs = Vec::new();

    for raw_command in commands {
        let command = raw_command.trim().to_string();
        if command.is_empty() {
            continue;
        }

        logs.push(format!("$ {}", command));

        if let Some(target_cwd) = parse_cd_command(&cwd, &command) {
            match target_cwd {
                Ok(path) => {
                    cwd = path.clone();
                    logs.push(format!("Changed directory to {}", cwd));
                }
                Err(error) => {
                    logs.push(format!("cd: {}", error));
                    break;
                }
            }
            continue;
        }

        match run_shell_command(&cwd, &command) {
            Ok(output) => {
                if !output.trim().is_empty() {
                    logs.push(output);
                }
            }
            Err(error) => {
                logs.push(error);
                break;
            }
        }
    }

    CommandSequenceResult { cwd, logs }
}

fn append_output(output: &mut String, line: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(line);
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn default_terminal_cwd() -> String {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
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

fn parse_cd_command(current_cwd: &str, command: &str) -> Option<Result<String, String>> {
    let trimmed = command.trim();
    if !trimmed.starts_with("cd") {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let _ = parts.next();
    let target = parts.next().map(str::trim).unwrap_or("");
    let target = if target.is_empty() || target == "~" {
        std::env::var("HOME").unwrap_or_else(|_| current_cwd.to_string())
    } else {
        target.to_string()
    };

    let candidate = if Path::new(&target).is_absolute() {
        PathBuf::from(&target)
    } else {
        Path::new(current_cwd).join(&target)
    };
    let normalized = match candidate.canonicalize() {
        Ok(path) => path,
        Err(error) => return Some(Err(format!("{} ({})", target, error))),
    };
    if !normalized.is_dir() {
        return Some(Err(format!("{} is not a directory", normalized.display())));
    }
    Some(Ok(normalized.to_string_lossy().to_string()))
}

fn run_shell_command(cwd: &str, command: &str) -> Result<String, String> {
    let mut process = if cfg!(target_os = "windows") {
        let mut cmd = StdCommand::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = StdCommand::new("/bin/zsh");
        cmd.arg("-lc").arg(command);
        cmd
    };

    if !cwd.trim().is_empty() {
        process.current_dir(cwd);
    }

    let output = process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Failed to spawn command: {}", error))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let mut text = String::new();
    if !stdout.is_empty() {
        text.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if text.is_empty() {
        text = if output.status.success() {
            format!("Command completed in {}", cwd)
        } else {
            format!("Command exited with status {}", output.status)
        };
    }

    if output.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}
