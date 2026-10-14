//! Opt-in native menu locale regression using the production Settings callback.
use slint::{ComponentHandle, Timer, Weak};
use std::path::PathBuf;
use std::time::Duration;

use crate::{AppState, MainWindow};

const STAGES: [(&str, &str); 3] = [("en", "Quit"), ("zh-CN", "退出"), ("en", "Quit")];

pub(super) fn start(window: Weak<MainWindow>) {
    if std::env::var("USHER_TRAY_LOCALE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let isolated_root = std::env::var_os("USHER_BENCHMARK_STATE_ROOT")
        .map(PathBuf::from)
        .is_some_and(|root| root.is_absolute());
    if !crate::app_paths::benchmark_offline_mode() || !isolated_root {
        fail("offline mode and an absolute benchmark state root are required");
        return;
    }
    Timer::single_shot(
        Duration::from_millis(100),
        move || match super::macos::menu_locale_snapshot() {
            Ok((menu, _)) => run_stage(window, menu, 0),
            Err(error) => fail(&error.to_string()),
        },
    );
}

fn run_stage(window: Weak<MainWindow>, original_menu: usize, stage: usize) {
    let Some(ui) = window.upgrade() else {
        fail("main window is unavailable");
        return;
    };
    let (locale, expected) = STAGES[stage];
    // Exercise the root callback, including persistence under the isolated root.
    ui.global::<AppState>().invoke_set_language(locale.into());
    let (menu, title) = match super::macos::menu_locale_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            fail(&error.to_string());
            return;
        }
    };
    if menu != original_menu || title != expected {
        fail(&format!(
            "stage={} locale={locale} menu_same={} expected={expected:?} observed={title:?}",
            stage + 1,
            menu == original_menu,
        ));
        return;
    }
    eprintln!(
        "[Tray][LocaleFixture] PASS stage={} locale={locale} title={expected} menu_same=true",
        stage + 1,
    );
    if stage + 1 < STAGES.len() {
        Timer::single_shot(Duration::from_millis(100), move || {
            run_stage(window, original_menu, stage + 1);
        });
    } else {
        eprintln!("[Tray][LocaleFixture] PASS complete stages=3 existing_menu=true");
        schedule_shutdown();
    }
}

fn fail(reason: &str) {
    eprintln!("[Tray][LocaleFixture] FAIL {reason}");
    schedule_shutdown();
}

fn schedule_shutdown() {
    Timer::single_shot(Duration::from_millis(100), || {
        crate::shutdown::request_shutdown("tray-locale-fixture");
    });
}
