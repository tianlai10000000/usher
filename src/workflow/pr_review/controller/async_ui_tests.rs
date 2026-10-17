//! Isolated real event-loop controls for asynchronous metadata failure and idle cleanup.
use super::super::*;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CHILD: &str =
    "workflow::pr_review::controller::lifecycle::async_ui_tests::metadata_failure_event_loop_child";

#[test]
fn actual_review_open_reload_and_close_use_queued_idle_checks_in_both_locales() {
    for locale in ["en", "zh-CN"] {
        for mode in ["open", "reload", "close"] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", CHILD, "--nocapture"])
                .env("USHER_PR_IDLE_UI_TEST_MODE", mode)
                .env("USHER_PR_IDLE_UI_TEST_LOCALE", locale)
                .env_remove("USHER_AI_REVIEW_FIXTURE")
                .env_remove("USHER_P6_BENCHMARK")
                .env_remove("USHER_MEMORY_PR_REVIEW_FIXTURE")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut timed_out = false;
            while child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap(); // Reap even a timed-out child.
            assert!(
                !timed_out && output.status.success(),
                "{locale}/{mode}, timeout={timed_out}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
#[ignore = "blocking parent test runs this in a dedicated event-loop process per locale/mode"]
fn metadata_failure_event_loop_child() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    let locale = std::env::var("USHER_PR_IDLE_UI_TEST_LOCALE").unwrap();
    let mode = std::env::var("USHER_PR_IDLE_UI_TEST_MODE").unwrap();
    let ui = MainWindow::new().unwrap();
    crate::i18n::apply_preference(&locale).unwrap();
    let weak = ui.as_weak();
    ui.on_navigate(move |route| {
        weak.upgrade().unwrap().set_current_route(route);
    });
    let list = super::super::super::super::pr_list::bind_pr_list(&ui);
    let controller = bind_pr_review(&ui, list);
    *controller.project_alias.lock().unwrap() = "missing-unit-test-project".into();
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_tab(2);
    // The isolated child intentionally has no registered Project Store. Real
    // open/Reload uses the shared spawn_blocking metadata chain and its error
    // callback; it cannot read workstation project/settings configuration or HTTP.
    assert!(crate::project::load_repository_metadata("missing-unit-test-project").is_err());
    let cache = controller.highlighting.resource_cache().clone();
    // Setup happens before the event loop, not during a product interaction.
    // No strong graph/slot handle is retained outside the cache after this line.
    let grammar = Arc::downgrade(
        cache
            .lease()
            .get_or_init(|| Arc::new(HighlightResources::load())),
    );
    assert!(cache.is_loaded());
    let request = ReviewRequest {
        number: "7".into(),
        title: "isolated async metadata".into(),
        author: "fixture".into(),
        source_branch: "head".into(),
        target_branch: "base".into(),
        updated_at: String::new(),
        ignore_whitespace: false,
    };
    match mode.as_str() {
        "open" => ui
            .pr_review()
            .global::<crate::PrReviewBridge>()
            .invoke_workflow_pr_review_open(
                request.number.clone().into(),
                request.title.clone().into(),
                request.author.clone().into(),
                request.source_branch.clone().into(),
                request.target_branch.clone().into(),
                "".into(),
                2,
            ),
        "reload" => {
            *controller.active_request.lock().unwrap() = Some(request);
            ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .invoke_workflow_pr_review_reload(false);
        }
        "close" => controller.clear_owned_state(&ui),
        _ => panic!("unknown isolated test mode"),
    }
    let inspected = Arc::new(AtomicBool::new(false));
    let inspection = inspected.clone();
    let observed_cache = cache.clone();
    let observed_grammar = grammar.clone();
    let closed = mode == "close";
    // The first reset check was queued inside the actual callback above. Even
    // if metadata already queued its error callback, that callback cannot queue
    // its second idle check until after this inspection is already enqueued.
    slint::invoke_from_event_loop(move || {
        assert_eq!(observed_cache.is_loaded(), !closed);
        if !closed {
            assert!(
                observed_grammar.upgrade().is_some(),
                "open must claim before async metadata"
            );
        }
        inspection.store(true, Ordering::Release);
    })
    .unwrap();
    let passed = Arc::new(AtomicBool::new(false));
    let completed = passed.clone();
    let weak = ui.as_weak();
    let observed_cache = cache.clone();
    let observed_grammar = grammar.clone();
    let inspected_timer = inspected.clone();
    let error_key = GitError::configuration("isolated metadata failure").user_message_key();
    let expected_error = crate::i18n::tr(error_key).into_owned();
    let deadline = Instant::now() + Duration::from_secs(5);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(5),
        move || {
            let ui = weak.upgrade().unwrap();
            let error_complete = closed || {
                let page = ui.pr_review();
                let bridge = page.global::<crate::PrReviewBridge>();
                !bridge.get_workflow_pr_review_loading()
                    && bridge.get_workflow_pr_review_error() == expected_error.as_str()
            };
            if inspected_timer.load(Ordering::Acquire)
                && error_complete
                && !observed_cache.is_loaded()
                && observed_grammar.strong_count() == 0
            {
                completed.store(true, Ordering::Release);
                slint::quit_event_loop().unwrap();
            } else if Instant::now() >= deadline {
                slint::quit_event_loop().unwrap();
            }
        },
    );
    ui.show().unwrap();
    slint::run_event_loop().unwrap();
    timer.stop();
    assert!(
        inspected.load(Ordering::Acquire),
        "queued inspection was not delivered"
    );
    assert!(
        passed.load(Ordering::Acquire),
        "{locale}/{mode}: current error/idle release timed out"
    );
    assert!(!cache.is_loaded());
    assert_eq!(grammar.strong_count(), 0);
}
