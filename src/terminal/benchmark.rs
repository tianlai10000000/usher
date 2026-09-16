//! Offline-only real PTY fixture, identical in baseline and candidate builds.
use super::*;

pub(super) fn setup(ui: &MainWindow) {
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

fn heartbeat(weak: slint::Weak<MainWindow>, start: Instant, last: Instant) {
    Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || start.elapsed() > Duration::from_secs(26) {
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
        if cycle < 2 {
            // Hide/reopen preserves the session; project replacement starts a fresh one.
            if cycle == 0 {
                ui.invoke_shell_drawer_close();
                ui.set_shell_drawer_visible(false);
                open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            } else {
                open_terminal_for_project(&ui, "P6 Terminal B", "p6-b", "/tmp");
            }
            run_cycle(weak, cycle + 1);
        } else {
            open_terminal_for_project(&ui, "P6 Terminal", "p6-a", "/tmp");
            log::info!("[P6][65] complete");
        }
    });
}
