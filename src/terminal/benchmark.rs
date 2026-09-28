//! Offline-only real PTY fixture, identical in baseline and candidate builds.
use super::*;

pub(super) fn setup(ui: &MainWindow) {
    #[cfg(feature = "test-fixtures")]
    if setup_home_return_attribution(ui) {
        return;
    }

    if std::env::var("USHER_P6_BENCHMARK").as_deref() != Ok("65")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
            ui.set_current_route(Route::Project);
            open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            heartbeat(ui.as_weak(), Instant::now(), Instant::now());
            run_cycle(ui.as_weak(), 0);
        });
    });
}

#[cfg(feature = "test-fixtures")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HomeReturnArm {
    WorkflowOnly,
    Terminal,
}

#[cfg(feature = "test-fixtures")]
impl HomeReturnArm {
    fn from_environment() -> Option<Self> {
        match std::env::var("USHER_MEMORY_TERMINAL_HOME_ARM")
            .ok()?
            .as_str()
        {
            "workflow-only" => Some(Self::WorkflowOnly),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::WorkflowOnly => "workflow-only",
            Self::Terminal => "terminal",
        }
    }
}

#[cfg(feature = "test-fixtures")]
#[derive(Clone, Copy)]
struct HomeReturnTiming {
    settle: Duration,
    sample_window: Duration,
    active: Duration,
}

#[cfg(feature = "test-fixtures")]
fn seconds_from_environment(name: &str, default: u64, minimum: u64) -> Option<Duration> {
    let seconds = match std::env::var(name) {
        Ok(value) => value.parse::<u64>().ok()?,
        Err(_) => default,
    };
    (seconds >= minimum).then(|| Duration::from_secs(seconds))
}

#[cfg(feature = "test-fixtures")]
fn setup_home_return_attribution(ui: &MainWindow) -> bool {
    if std::env::var_os("USHER_MEMORY_TERMINAL_HOME_ARM").is_none() {
        return false;
    }

    let arm = HomeReturnArm::from_environment();
    let timing = seconds_from_environment("USHER_MEMORY_SETTLE_SECONDS", 30, 1)
        .zip(seconds_from_environment(
            "USHER_MEMORY_SAMPLE_WINDOW_SECONDS",
            20,
            2,
        ))
        .zip(seconds_from_environment(
            "USHER_MEMORY_ACTIVE_SECONDS",
            6,
            3,
        ))
        .map(|((settle, sample_window), active)| HomeReturnTiming {
            settle,
            sample_window,
            active,
        });
    if arm.is_none()
        || timing.is_none()
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
        || !crate::memory_benchmark::nonactivating_window_requested()
    {
        log::error!(
            "[Memory][TerminalHome] invalid fixture configuration; refusing interactive fallback"
        );
        crate::startup::run_after_first_frame(|| {
            crate::shutdown::request_shutdown("terminal-home-invalid-fixture");
        });
        return true;
    }

    let arm = arm.expect("validated arm");
    let timing = timing.expect("validated timing");
    ui.window().set_size(slint::LogicalSize::new(800.0, 600.0));
    ui.set_shell_drawer_visible(false);
    ui.set_shell_drawer_fullscreen(false);

    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        crate::memory_benchmark::configure_noninteractive_window(&ui);
        ui.invoke_navigate(Route::Home);
        log::info!(
            "[Memory][TerminalHome] event=fixture_started arm={} settle_seconds={} sample_window_seconds={} active_seconds={} input=event-loop-only",
            arm.label(),
            timing.settle.as_secs(),
            timing.sample_window.as_secs(),
            timing.active.as_secs(),
        );
        Timer::single_shot(timing.settle, move || {
            publish_home_phase(ui.as_weak(), arm, timing, 0);
        });
    });
    true
}

#[cfg(feature = "test-fixtures")]
fn publish_home_phase(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    timing: HomeReturnTiming,
    completed_round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let (session_present, connected, running, rendered_lines, project_key) = TERMINAL_SHARED_STATE
        .get()
        .and_then(|state| state.lock().ok())
        .map(|state| {
            (
                state.session.is_some(),
                state.connected,
                state.running,
                state.lines.len(),
                state.project_key.clone(),
            )
        })
        .unwrap_or((false, false, false, 0, String::new()));
    let expected_session = arm == HomeReturnArm::Terminal && completed_round > 0;
    let valid = ui.get_current_route() == Route::Home
        && !ui.get_shell_drawer_visible()
        && session_present == expected_session;
    let phase = if completed_round == 0 {
        "initial_home"
    } else if completed_round == 1 {
        "returned_home_1"
    } else {
        "returned_home_2"
    };
    log::info!(
        "[Memory][TerminalHome] event=phase_ready phase={phase} arm={} round={} route={:?} terminal_visible={} session_present={} connected={} running={} rendered_lines={} project_key={} valid={}",
        arm.label(),
        completed_round,
        ui.get_current_route(),
        ui.get_shell_drawer_visible(),
        session_present,
        connected,
        running,
        rendered_lines,
        if project_key.is_empty() {
            "none"
        } else {
            project_key.as_str()
        },
        valid,
    );
    crate::memory_diagnostics::log_checkpoint("terminal_home.phase_ready");

    Timer::single_shot(timing.sample_window, move || {
        if completed_round < 2 {
            start_home_return_round(ui.as_weak(), arm, timing, completed_round + 1);
        } else {
            log::info!(
                "[Memory][TerminalHome] event=fixture_complete arm={} rounds=2",
                arm.label()
            );
            crate::shutdown::request_shutdown("terminal-home-fixture-complete");
        }
    });
}

#[cfg(feature = "test-fixtures")]
fn start_home_return_round(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    timing: HomeReturnTiming,
    round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    ui.invoke_navigate(Route::Workflow);
    log::info!(
        "[Memory][TerminalHome] event=round_started arm={} round={} route={:?}",
        arm.label(),
        round,
        ui.get_current_route(),
    );
    if arm == HomeReturnArm::Terminal {
        open_terminal_for_project(&ui, "Memory Fixture", "terminal-home", "/tmp");
        schedule_terminal_fixture_submit(ui.as_weak(), round, Instant::now(), timing.active);
    }

    Timer::single_shot(timing.active, move || {
        finish_home_return_round(ui.as_weak(), arm, timing, round);
    });
}

#[cfg(feature = "test-fixtures")]
fn terminal_fixture_marker(round: usize) -> String {
    format!("USHER_TERM_D_{round}")
}

#[cfg(feature = "test-fixtures")]
fn schedule_terminal_fixture_submit(
    weak: slint::Weak<MainWindow>,
    round: usize,
    started: Instant,
    active: Duration,
) {
    Timer::single_shot(Duration::from_millis(100), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let connected = TERMINAL_SHARED_STATE
            .get()
            .and_then(|state| state.lock().ok())
            .is_some_and(|state| state.connected && state.session.is_some());
        if connected {
            let command = format!("printf '%s%s\\n' 'USHER_' 'TERM_D_{round}'");
            ui.invoke_shell_drawer_submit(command.into());
            log::info!("[Memory][TerminalHome] event=terminal_command_submitted round={round}");
            return;
        }
        if started.elapsed() + Duration::from_millis(500) < active {
            schedule_terminal_fixture_submit(weak, round, started, active);
        } else {
            log::error!("[Memory][TerminalHome] event=terminal_connect_timeout round={round}");
        }
    });
}

#[cfg(feature = "test-fixtures")]
fn finish_home_return_round(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    timing: HomeReturnTiming,
    round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let terminal_operation_complete = if arm == HomeReturnArm::Terminal {
        let marker = terminal_fixture_marker(round);
        let marker_present = TERMINAL_SHARED_STATE
            .get()
            .and_then(|state| state.lock().ok())
            .is_some_and(|state| state.render.rendered_text().contains(&marker));
        ui.invoke_shell_drawer_close();
        ui.set_shell_drawer_visible(false);
        ui.set_shell_drawer_fullscreen(false);
        marker_present
    } else {
        true
    };
    ui.invoke_navigate(Route::Home);
    log::info!(
        "[Memory][TerminalHome] event=returned_home arm={} round={} route={:?} terminal_visible={} terminal_operation_complete={}",
        arm.label(),
        round,
        ui.get_current_route(),
        ui.get_shell_drawer_visible(),
        terminal_operation_complete,
    );
    crate::memory_diagnostics::log_checkpoint("terminal_home.returned_home");
    Timer::single_shot(timing.settle, move || {
        publish_home_phase(ui.as_weak(), arm, timing, round);
    });
}

fn heartbeat(weak: slint::Weak<MainWindow>, start: Instant, last: Instant) {
    Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || !crate::p6_benchmark::heartbeat_active(26) {
            return;
        }
        log::info!("[P6][65] heartbeat_us={}", last.elapsed().as_micros());
        heartbeat(weak, start, Instant::now());
    });
}

fn run_cycle(weak: slint::Weak<MainWindow>, cycle: usize) {
    Timer::single_shot(Duration::from_millis(500), move || {
        let Some(ui) = weak.upgrade() else { return };
        // Split the marker so shell input echo cannot satisfy the completion check.
        let command = format!(
            "stty -echo; awk 'BEGIN {{ for(i=0;i<150000;i++) print \"0123456789 abcdefghijklmnopqrstuvwxyz 0123456789 abcdefghijklmnopqrstuvwxyz\"; print \"P6_\" \"DONE_{cycle}\"; }}'"
        );
        ui.invoke_shell_drawer_submit(command.into());
        await_tail(weak, cycle, Instant::now());
    });
}

fn await_tail(weak: slint::Weak<MainWindow>, cycle: usize, started: Instant) {
    Timer::single_shot(Duration::from_millis(100), move || {
        let Some(ui) = weak.upgrade() else { return };
        let tail = TERMINAL_SHARED_STATE
            .get()
            .and_then(|shared| shared.lock().ok())
            .is_some_and(|state| {
                state
                    .render
                    .rendered_text()
                    .contains(&format!("P6_DONE_{cycle}"))
            });
        if !tail {
            if started.elapsed() < Duration::from_secs(20) {
                await_tail(weak, cycle, started);
            }
            return;
        }
        log::info!(
            "[P6][65] cycle={cycle} completed_ms={}",
            started.elapsed().as_millis()
        );
        if crate::p6_benchmark::next_cycle(cycle + 1) {
            // Hide/reopen preserves the session; project replacement starts a fresh one.
            if cycle % 3 == 0 {
                ui.invoke_shell_drawer_close();
                ui.set_shell_drawer_visible(false);
                open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            } else if cycle % 3 == 1 {
                open_terminal_for_project(&ui, "P6 Terminal B", "p6-b", "/tmp");
            } else {
                open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            }
            run_cycle(weak, cycle + 1);
        } else {
            open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            log::info!("[P6][65] complete");
        }
    });
}
