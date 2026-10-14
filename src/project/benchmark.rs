//! Opt-in P5-B fixture. Uses the real Store actions, callbacks and Slint model.
use super::state::ProjectStateFacade;
use crate::{MainWindow, Route};
#[cfg(feature = "test-fixtures")]
use slint::Timer;
use slint::{ComponentHandle, Model};
use std::time::{Duration, Instant};

pub(super) fn setup(ui: &MainWindow, state: &ProjectStateFacade) {
    #[cfg(feature = "test-fixtures")]
    if setup_home_return_attribution(ui, state) {
        return;
    }

    if std::env::var("USHER_P5_BENCHMARK").as_deref() != Ok("57")
        || std::env::var("USHER_BENCHMARK_OFFLINE").as_deref() != Ok("1")
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let window = ui.as_weak();
    let state = state.clone();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn(async move {
            // Wait for real startup hydration before exercising projection.
            while state.snapshot().state().projects.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let weak = window.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_current_route(Route::Project);
                }
            });
            tokio::time::sleep(Duration::from_secs(2)).await;
            for round in 0..24u32 {
                let began = Instant::now();
                for index in 0..64 {
                    state.complete_pr_overview(format!("p5-{index}"), Some((round, index, 1)));
                }
                let weak = window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        let query = if round % 2 == 0 { "project 1" } else { "" };
                        ui.global::<crate::ProjectBridge>()
                            .set_search_query(query.into());
                        ui.global::<crate::ProjectBridge>()
                            .invoke_search_projects(query.into());
                        log::info!(
                            "[P5][57] round={round} dispatch_to_ui_us={}",
                            began.elapsed().as_micros()
                        );
                    }
                });
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = window.upgrade() {
                    let model = ui.global::<crate::ProjectBridge>().get_items();
                    let updated = (0..model.row_count())
                        .filter(|&i| {
                            model
                                .row_data(i)
                                .is_some_and(|row| row.pr_fat_count_text == "23")
                        })
                        .count();
                    log::info!(
                        "[P5][57] complete rows={} updated={updated}",
                        model.row_count()
                    );
                }
            });
        });
    });
}

#[cfg(feature = "test-fixtures")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HomeReturnArm {
    HomeOnly,
    Empty,
    DefaultLogo,
    SixLogos,
}

#[cfg(feature = "test-fixtures")]
impl HomeReturnArm {
    fn from_environment() -> Option<Self> {
        match std::env::var("USHER_MEMORY_PROJECT_HOME_ARM")
            .ok()?
            .as_str()
        {
            "home-only" => Some(Self::HomeOnly),
            "empty" => Some(Self::Empty),
            "default-logo" => Some(Self::DefaultLogo),
            "six-logos" => Some(Self::SixLogos),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::HomeOnly => "home-only",
            Self::Empty => "empty",
            Self::DefaultLogo => "default-logo",
            Self::SixLogos => "six-logos",
        }
    }

    fn expected_rows(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::HomeOnly | Self::DefaultLogo | Self::SixLogos => 10,
        }
    }

    fn expected_filtered_rows(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::HomeOnly | Self::DefaultLogo | Self::SixLogos => 1,
        }
    }

    fn enters_project(self) -> bool {
        self != Self::HomeOnly
    }
}

#[cfg(feature = "test-fixtures")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HomeReturnInteraction {
    GridOnly,
    GridList,
}

#[cfg(feature = "test-fixtures")]
impl HomeReturnInteraction {
    fn from_label(label: &str) -> Option<Self> {
        match label {
            "grid-only" => Some(Self::GridOnly),
            "grid-list" => Some(Self::GridList),
            _ => None,
        }
    }

    fn from_environment() -> Option<Self> {
        Self::from_label(
            std::env::var("USHER_MEMORY_PROJECT_HOME_INTERACTION")
                .ok()?
                .as_str(),
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::GridOnly => "grid-only",
            Self::GridList => "grid-list",
        }
    }

    fn visits_list(self) -> bool {
        self == Self::GridList
    }
}

#[cfg(feature = "test-fixtures")]
#[derive(Clone, Copy)]
struct HomeReturnTiming {
    settle: Duration,
    sample_window: Duration,
    active: Duration,
    active_sample: Duration,
}

#[cfg(feature = "test-fixtures")]
impl HomeReturnTiming {
    fn operation_delay(self, base_seconds: u64) -> Duration {
        Duration::from_secs(base_seconds) + self.active_sample
    }

    fn active_sampling_enabled(self) -> bool {
        !self.active_sample.is_zero()
    }

    fn is_valid(self) -> bool {
        let active_sample_is_valid =
            self.active_sample.is_zero() || self.active_sample >= Duration::from_secs(12);
        active_sample_is_valid && self.active == Duration::from_secs(8) + self.active_sample
    }
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
fn setup_home_return_attribution(ui: &MainWindow, state: &ProjectStateFacade) -> bool {
    if std::env::var_os("USHER_MEMORY_PROJECT_HOME_ARM").is_none() {
        return false;
    }

    let arm = HomeReturnArm::from_environment();
    let interaction = HomeReturnInteraction::from_environment();
    let timing = seconds_from_environment("USHER_MEMORY_SETTLE_SECONDS", 30, 1)
        .zip(seconds_from_environment(
            "USHER_MEMORY_SAMPLE_WINDOW_SECONDS",
            20,
            2,
        ))
        .zip(seconds_from_environment(
            "USHER_MEMORY_ACTIVE_SECONDS",
            8,
            8,
        ))
        .zip(seconds_from_environment(
            "USHER_MEMORY_ACTIVE_SAMPLE_SECONDS",
            0,
            0,
        ))
        .map(
            |(((settle, sample_window), active), active_sample)| HomeReturnTiming {
                settle,
                sample_window,
                active,
                active_sample,
            },
        );
    if arm.is_none()
        || interaction.is_none()
        || timing.is_none()
        || timing.is_some_and(|timing| !timing.is_valid())
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
        || !crate::memory_benchmark::nonactivating_window_requested()
    {
        log::error!(
            "[Memory][ProjectHome] invalid fixture configuration; refusing interactive fallback"
        );
        crate::startup::run_after_first_frame(|| {
            crate::shutdown::request_shutdown("project-home-invalid-fixture");
        });
        return true;
    }

    let arm = arm.expect("validated arm");
    let interaction = interaction.expect("validated interaction");
    let timing = timing.expect("validated timing");
    ui.window().set_size(slint::LogicalSize::new(800.0, 600.0));
    ui.global::<crate::ProjectBridge>()
        .set_show_add_modal(false);
    ui.global::<crate::ProjectBridge>()
        .set_search_query("".into());
    ui.global::<crate::ProjectBridge>()
        .set_view_mode("grid".into());

    let weak = ui.as_weak();
    let state = state.clone();
    crate::startup::run_after_first_frame(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        crate::memory_benchmark::configure_noninteractive_window(&ui);
        ui.invoke_navigate(Route::Home);
        let state_dir = crate::app_paths::config_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unavailable".to_owned());
        log::info!(
            "[Memory][ProjectHome] event=fixture_started arm={} interaction={} expected_rows={} settle_seconds={} sample_window_seconds={} active_seconds={} active_sample_seconds={} input=event-loop-only state_dir={}",
            arm.label(),
            interaction.label(),
            arm.expected_rows(),
            timing.settle.as_secs(),
            timing.sample_window.as_secs(),
            timing.active.as_secs(),
            timing.active_sample.as_secs(),
            state_dir,
        );
        wait_for_projection(
            ui.as_weak(),
            state,
            arm,
            interaction,
            timing,
            Instant::now(),
        );
    });
    true
}

#[cfg(feature = "test-fixtures")]
fn wait_for_projection(
    weak: slint::Weak<MainWindow>,
    state: ProjectStateFacade,
    arm: HomeReturnArm,
    interaction: HomeReturnInteraction,
    timing: HomeReturnTiming,
    started: Instant,
) {
    Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let snapshot = state.snapshot();
        let hydrated_rows = snapshot.state().projects.len();
        let projected_rows = ui.global::<crate::ProjectBridge>().get_items().row_count();
        let ready = snapshot.state().hydrated
            && hydrated_rows == arm.expected_rows()
            && projected_rows == arm.expected_rows()
            && !ui.global::<crate::ProjectBridge>().get_list_loading();
        if ready {
            log::info!(
                "[Memory][ProjectHome] event=projection_ready arm={} hydrated_rows={} projected_rows={}",
                arm.label(),
                hydrated_rows,
                projected_rows,
            );
            Timer::single_shot(timing.settle, move || {
                publish_home_phase(ui.as_weak(), arm, interaction, timing, 0);
            });
        } else if started.elapsed() < Duration::from_secs(20) {
            wait_for_projection(ui.as_weak(), state, arm, interaction, timing, started);
        } else {
            log::error!(
                "[Memory][ProjectHome] event=projection_timeout arm={} hydrated={} hydrated_rows={} projected_rows={} loading={}",
                arm.label(),
                snapshot.state().hydrated,
                hydrated_rows,
                projected_rows,
                ui.global::<crate::ProjectBridge>().get_list_loading(),
            );
            crate::shutdown::request_shutdown("project-home-projection-timeout");
        }
    });
}

#[cfg(feature = "test-fixtures")]
fn publish_home_phase(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    interaction: HomeReturnInteraction,
    timing: HomeReturnTiming,
    completed_round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let phase = if completed_round == 0 {
        "initial_home"
    } else if completed_round == 1 {
        "returned_home_1"
    } else {
        "returned_home_2"
    };
    let projected_rows = ui.global::<crate::ProjectBridge>().get_items().row_count();
    let valid = ui.get_current_route() == Route::Home
        && !ui.global::<crate::ProjectBridge>().get_show_add_modal()
        && ui
            .global::<crate::ProjectBridge>()
            .get_search_query()
            .is_empty()
        && ui.global::<crate::ProjectBridge>().get_view_mode().as_str() == "grid"
        && projected_rows == arm.expected_rows();
    log::info!(
        "[Memory][ProjectHome] event=phase_ready phase={phase} arm={} interaction={} round={} route={:?} projected_rows={} modal_visible={} query_empty={} view_mode={} valid={}",
        arm.label(),
        interaction.label(),
        completed_round,
        ui.get_current_route(),
        projected_rows,
        ui.global::<crate::ProjectBridge>().get_show_add_modal(),
        ui.global::<crate::ProjectBridge>()
            .get_search_query()
            .is_empty(),
        ui.global::<crate::ProjectBridge>().get_view_mode(),
        valid,
    );
    crate::memory_diagnostics::log_checkpoint("project_home.phase_ready");

    Timer::single_shot(timing.sample_window, move || {
        if completed_round < 2 {
            start_home_return_round(ui.as_weak(), arm, interaction, timing, completed_round + 1);
        } else {
            log::info!(
                "[Memory][ProjectHome] event=fixture_complete arm={} interaction={} rounds=2",
                arm.label(),
                interaction.label(),
            );
            crate::shutdown::request_shutdown("project-home-fixture-complete");
        }
    });
}

#[cfg(feature = "test-fixtures")]
fn start_home_return_round(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    interaction: HomeReturnInteraction,
    timing: HomeReturnTiming,
    round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    ui.global::<crate::ProjectBridge>()
        .set_search_query("".into());
    ui.global::<crate::ProjectBridge>()
        .set_view_mode("grid".into());
    if arm.enters_project() {
        ui.invoke_navigate(Route::Project);
    }
    log::info!(
        "[Memory][ProjectHome] event=round_started arm={} interaction={} round={} route={:?} projected_rows={}",
        arm.label(),
        interaction.label(),
        round,
        ui.get_current_route(),
        ui.global::<crate::ProjectBridge>().get_items().row_count(),
    );

    let view = ui.as_weak();
    Timer::single_shot(Duration::from_secs(2), move || {
        if let Some(ui) = view.upgrade() {
            if interaction.visits_list() {
                ui.global::<crate::ProjectBridge>()
                    .set_view_mode("list".into());
                log::info!(
                    "[Memory][ProjectHome] event=view_changed arm={} interaction=grid-list round={round} mode=list",
                    arm.label(),
                );
            } else {
                ui.global::<crate::ProjectBridge>()
                    .set_view_mode("grid".into());
                log::info!(
                    "[Memory][ProjectHome] event=view_changed arm={} interaction=grid-only round={round} mode=grid",
                    arm.label(),
                );
            }
        }
    });
    if timing.active_sampling_enabled() {
        let active = ui.as_weak();
        Timer::single_shot(Duration::from_secs(3), move || {
            publish_active_phase(active, arm, interaction, round);
        });
    }
    let search = ui.as_weak();
    Timer::single_shot(timing.operation_delay(4), move || {
        if let Some(ui) = search.upgrade() {
            ui.global::<crate::ProjectBridge>()
                .set_search_query("Project 1".into());
            ui.global::<crate::ProjectBridge>()
                .invoke_search_projects("Project 1".into());
            log::info!(
                "[Memory][ProjectHome] event=search_applied arm={} interaction={} round={round}",
                arm.label(),
                interaction.label(),
            );
        }
    });
    let verify_search = ui.as_weak();
    Timer::single_shot(timing.operation_delay(5), move || {
        if let Some(ui) = verify_search.upgrade() {
            let projected_rows = ui.global::<crate::ProjectBridge>().get_items().row_count();
            let expected_rows = arm.expected_filtered_rows();
            let valid = ui
                .global::<crate::ProjectBridge>()
                .get_search_query()
                .as_str()
                == "Project 1"
                && projected_rows == expected_rows;
            log::info!(
                "[Memory][ProjectHome] event=search_projection_checked arm={} interaction={} round={round} projected_rows={} expected_rows={} valid={valid}",
                arm.label(),
                interaction.label(),
                projected_rows,
                expected_rows,
            );
            if !valid {
                crate::shutdown::request_shutdown("project-home-search-projection-invalid");
            }
        }
    });
    let restore = ui.as_weak();
    Timer::single_shot(timing.operation_delay(6), move || {
        if let Some(ui) = restore.upgrade() {
            ui.global::<crate::ProjectBridge>()
                .set_search_query("".into());
            ui.global::<crate::ProjectBridge>()
                .invoke_search_projects("".into());
            ui.global::<crate::ProjectBridge>()
                .set_view_mode("grid".into());
            log::info!(
                "[Memory][ProjectHome] event=search_cleared arm={} interaction={} round={round} mode=grid",
                arm.label(),
                interaction.label(),
            );
        }
    });
    Timer::single_shot(timing.active, move || {
        finish_home_return_round(ui.as_weak(), arm, interaction, timing, round);
    });
}

#[cfg(feature = "test-fixtures")]
fn publish_active_phase(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    interaction: HomeReturnInteraction,
    round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let expected_mode = if interaction.visits_list() {
        "list"
    } else {
        "grid"
    };
    let projected_rows = ui.global::<crate::ProjectBridge>().get_items().row_count();
    let valid = arm.enters_project()
        && ui.get_current_route() == Route::Project
        && projected_rows == arm.expected_rows()
        && !ui.global::<crate::ProjectBridge>().get_show_add_modal()
        && ui
            .global::<crate::ProjectBridge>()
            .get_search_query()
            .is_empty()
        && ui.global::<crate::ProjectBridge>().get_view_mode().as_str() == expected_mode;
    log::info!(
        "[Memory][ProjectHome] event=phase_ready phase=active_project_{round} arm={} interaction={} round={} route={:?} projected_rows={} modal_visible={} query_empty={} view_mode={} valid={}",
        arm.label(),
        interaction.label(),
        round,
        ui.get_current_route(),
        projected_rows,
        ui.global::<crate::ProjectBridge>().get_show_add_modal(),
        ui.global::<crate::ProjectBridge>()
            .get_search_query()
            .is_empty(),
        ui.global::<crate::ProjectBridge>().get_view_mode(),
        valid,
    );
    crate::memory_diagnostics::log_checkpoint("project_home.active_phase_ready");
    if !valid {
        crate::shutdown::request_shutdown("project-home-active-phase-invalid");
    }
}

#[cfg(feature = "test-fixtures")]
fn finish_home_return_round(
    weak: slint::Weak<MainWindow>,
    arm: HomeReturnArm,
    interaction: HomeReturnInteraction,
    timing: HomeReturnTiming,
    round: usize,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let projected_rows = ui.global::<crate::ProjectBridge>().get_items().row_count();
    let expected_route = if arm.enters_project() {
        Route::Project
    } else {
        Route::Home
    };
    let operation_complete = ui.get_current_route() == expected_route
        && projected_rows == arm.expected_rows()
        && ui
            .global::<crate::ProjectBridge>()
            .get_search_query()
            .is_empty()
        && ui.global::<crate::ProjectBridge>().get_view_mode().as_str() == "grid";
    ui.invoke_navigate(Route::Home);
    log::info!(
        "[Memory][ProjectHome] event=returned_home arm={} interaction={} round={} route={:?} projected_rows={} operation_complete={}",
        arm.label(),
        interaction.label(),
        round,
        ui.get_current_route(),
        projected_rows,
        operation_complete,
    );
    crate::memory_diagnostics::log_checkpoint("project_home.returned_home");
    Timer::single_shot(timing.settle, move || {
        publish_home_phase(ui.as_weak(), arm, interaction, timing, round);
    });
}

#[cfg(all(test, feature = "test-fixtures"))]
mod home_return_tests {
    use super::{HomeReturnArm, HomeReturnInteraction, HomeReturnTiming};
    use std::time::Duration;

    #[test]
    fn fixture_arms_have_explicit_row_counts() {
        assert_eq!(HomeReturnArm::HomeOnly.expected_rows(), 10);
        assert_eq!(HomeReturnArm::Empty.expected_rows(), 0);
        assert_eq!(HomeReturnArm::DefaultLogo.expected_rows(), 10);
        assert_eq!(HomeReturnArm::SixLogos.expected_rows(), 10);
        assert_eq!(HomeReturnArm::HomeOnly.expected_filtered_rows(), 1);
        assert_eq!(HomeReturnArm::Empty.expected_filtered_rows(), 0);
        assert!(!HomeReturnArm::HomeOnly.enters_project());
        assert!(HomeReturnArm::Empty.enters_project());
        assert_ne!(
            HomeReturnArm::DefaultLogo.label(),
            HomeReturnArm::SixLogos.label()
        );
    }

    #[test]
    fn fixture_interactions_are_explicit_and_distinct() {
        assert_eq!(
            HomeReturnInteraction::from_label("grid-only"),
            Some(HomeReturnInteraction::GridOnly)
        );
        assert_eq!(
            HomeReturnInteraction::from_label("grid-list"),
            Some(HomeReturnInteraction::GridList)
        );
        assert_eq!(HomeReturnInteraction::from_label("list-only"), None);
        assert!(!HomeReturnInteraction::GridOnly.visits_list());
        assert!(HomeReturnInteraction::GridList.visits_list());
        assert_ne!(
            HomeReturnInteraction::GridOnly.label(),
            HomeReturnInteraction::GridList.label()
        );
    }

    #[test]
    fn fixture_timing_reserves_the_active_sampling_window() {
        let legacy = HomeReturnTiming {
            settle: Duration::from_secs(30),
            sample_window: Duration::from_secs(25),
            active: Duration::from_secs(8),
            active_sample: Duration::ZERO,
        };
        assert!(legacy.is_valid());
        assert_eq!(legacy.operation_delay(4), Duration::from_secs(4));

        let sampled = HomeReturnTiming {
            active: Duration::from_secs(23),
            active_sample: Duration::from_secs(15),
            ..legacy
        };
        assert!(sampled.is_valid());
        assert!(sampled.active_sampling_enabled());
        assert_eq!(sampled.operation_delay(4), Duration::from_secs(19));

        let too_short = HomeReturnTiming {
            active: Duration::from_secs(18),
            active_sample: Duration::from_secs(10),
            ..legacy
        };
        assert!(!too_short.is_valid());
    }
}
