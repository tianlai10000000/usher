//! Isolated event-loop fixtures exercise the compiled Project card/row chain.
use super::*;
use i_slint_backend_testing::ElementHandle;
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;
const FIXTURE: &str = "workflow::state::facade::ui_tests::project_admission_event_loop_fixture";
#[test]
fn compiled_project_open_waits_for_commit_and_rejected_or_stale_requests_do_not_navigate() {
    for locale in ["en", "zh-CN"] {
        for mode in ["grid", "list"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", FIXTURE, "--nocapture"])
                .env("USHER_WORKFLOW_TEST_LOCALE", locale)
                .env("USHER_WORKFLOW_TEST_MODE", mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{locale}/{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
#[test]
#[ignore = "runs in a dedicated event-loop process for each locale/layout"]
fn project_admission_event_loop_fixture() {
    // Pointer helpers and route remount checks advance Slint's mock clock.
    // Using the system-time backend with those helpers can rewind animation
    // time on the next event-loop turn.
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let locale = std::env::var("USHER_WORKFLOW_TEST_LOCALE").unwrap();
    let mode = std::env::var("USHER_WORKFLOW_TEST_MODE").unwrap();
    let ui = crate::MainWindow::new().unwrap();
    crate::i18n::apply_preference(&locale).unwrap();
    ui.window().set_size(LogicalSize::new(1200.0, 900.0));
    ui.set_platform_is_macos(false);
    ui.set_show_breadcrumb(false);
    ui.set_show_navigation_drawer(false);
    ui.set_current_route(crate::Route::Project);
    let state = SharedWorkflowState::new();
    state.install(&ui);
    let rejected = Rc::new(Cell::new(0));
    let errors = rejected.clone();
    let facade = state.clone();
    ui.on_workflow_open_project(move |name, alias, local, remote, logo, _, _| {
        let context = WorkflowProjectContext {
            id: alias.to_string(),
            name: name.to_string(),
            alias: alias.to_string(),
            local_repo: local.to_string(),
            remote_repo: remote.to_string(),
            logo: logo.to_string(),
        };
        if facade.select_project(context).is_err() {
            errors.set(errors.get() + 1);
        }
    });
    let navigations = Rc::new(Cell::new(0));
    let observed = navigations.clone();
    let weak = ui.as_weak();
    ui.on_navigate(move |route| {
        let ui = weak.upgrade().unwrap();
        assert_eq!(route, crate::Route::Workflow);
        assert_eq!(ui.get_workflow_project_id(), "c");
        assert_eq!(ui.get_workflow_project_name(), "Project C");
        observed.set(observed.get() + 1);
        ui.set_current_route(route);
        slint::quit_event_loop().unwrap();
    });
    let projects = ui.global::<crate::ProjectBridge>();
    projects.set_view_mode(mode.into());
    projects.set_items(ModelRc::new(VecModel::from(vec![
        crate::ProjectItem {
            project_name: "Project B".into(),
            project_alias: "b".into(),
            ..Default::default()
        },
        crate::ProjectItem {
            project_name: "Project C".into(),
            project_alias: "c".into(),
            ..Default::default()
        },
    ])));
    ui.show().unwrap();
    fn click(ui: &crate::MainWindow, locale: &str, name: &str) {
        let label = format!(
            "{} {name}",
            usher_i18n::translate_for(locale, "projects.open_project")
        );
        ElementHandle::find_by_accessible_label(ui, &label)
            .into_iter()
            .last()
            .expect("compiled Project open action")
            .mock_single_click(PointerEventButton::Left);
    }
    let permits = (0..CAPACITY)
        .map(|_| state.inner.owners.clone().try_acquire_owned().unwrap())
        .collect::<Vec<_>>();
    click(&ui, &locale, "Project B");
    assert_eq!(rejected.get(), 1);
    assert_eq!(ui.get_current_route(), crate::Route::Project);
    assert_eq!(ui.get_workflow_project_id(), "default");
    drop(permits);
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let _subscription = state.inner.store.subscribe(Arc::new(move |snapshot| {
        if snapshot.state().current_project.id == "b" {
            entered.send(()).unwrap();
            blocked
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }));
    click(&ui, &locale, "Project B");
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(ui.get_current_route(), crate::Route::Project);
    assert_eq!(ui.get_workflow_project_id(), "default");
    click(&ui, &locale, "Project C");
    release.send(()).unwrap();
    crate::app_runtime::shared_runtime().block_on(async {
        let sender = state
            .inner
            .admission
            .lock()
            .unwrap()
            .sender
            .clone()
            .unwrap();
        let (done, wait) = tokio::sync::oneshot::channel();
        sender.send(Command::Barrier(done)).await.unwrap();
        wait.await.unwrap();
    });
    assert_eq!(ui.get_current_route(), crate::Route::Project);
    assert_eq!(state.snapshot().state().current_project.id, "c");
    assert_eq!(
        state.inner.owners.available_permits(),
        CAPACITY - 2,
        "committed project operations retain their owners until queued UI effects run"
    );
    // The mock-time event loop intentionally does not advance timers itself.
    // Keep the failure watchdog on a real worker clock, independent of the
    // Slint clock used by all four locale/layout cells.
    let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timeout = expired.clone();
    let (finished, completion) = mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if matches!(
            completion.recv_timeout(Duration::from_secs(3)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            timeout.store(true, std::sync::atomic::Ordering::Release);
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        }
    });
    slint::run_event_loop().unwrap();
    let _ = finished.send(());
    watchdog.join().unwrap();
    assert!(
        !expired.load(std::sync::atomic::Ordering::Acquire),
        "committed project did not navigate"
    );
    assert_eq!(navigations.get(), 1);
    assert_eq!(ui.get_workflow_project_id(), "c");
    assert_eq!(state.inner.owners.available_permits(), CAPACITY);
    crate::workflow::tag::bind_tag(&ui, state.clone());
    fn select_tags(ui: &crate::MainWindow, locale: &str) {
        let label = usher_i18n::translate_for(locale, "workflow.tags");
        ElementHandle::find_by_accessible_label(ui, label.as_ref())
            .into_iter()
            .last()
            .expect("compiled Tag nav action")
            .mock_single_click(PointerEventButton::Left);
    }
    fn description_input(ui: &crate::MainWindow, locale: &str) -> ElementHandle {
        let label = usher_i18n::translate_for(locale, "tag.description");
        ElementHandle::find_by_accessible_label(ui, label.as_ref())
            .into_iter()
            .find(|field| {
                field.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::TextInput)
            })
            .expect("compiled Tag description editor")
    }
    select_tags(&ui, &locale);
    let draft = "draft 中文 & $() survives remount";
    description_input(&ui, &locale).set_accessible_value(draft);
    crate::app_runtime::shared_runtime().block_on(async {
        let sender = state
            .inner
            .admission
            .lock()
            .unwrap()
            .sender
            .clone()
            .unwrap();
        let (done, wait) = tokio::sync::oneshot::channel();
        sender.send(Command::Barrier(done)).await.unwrap();
        wait.await.unwrap();
    });
    assert_eq!(
        state.snapshot().state().current_state_ref().tag_description,
        draft
    );
    ui.set_workflow_tag_selected_tab(1);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ui.set_workflow_tag_selected_tab(0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(
        description_input(&ui, &locale).accessible_value().unwrap(),
        draft
    );
    ui.set_current_route(crate::Route::Settings);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ui.set_current_route(crate::Route::Workflow);
    select_tags(&ui, &locale);
    assert_eq!(
        description_input(&ui, &locale).accessible_value().unwrap(),
        draft
    );
    crate::app_runtime::shared_runtime().block_on(state.shutdown_async());
}
