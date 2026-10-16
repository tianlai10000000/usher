//! Saved-profile identity, cancellation and real shared-HTTP probe fixtures.

use super::*;
use crate::MainWindow;
use crate::settings::{AiAgentConfiguration, AiAgentHeader, AiAgentSettings};
use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use std::time::Duration;

const SECRET: &str = "synthetic-list-probe-header";
const CONNECTED: &str = "settings.feedback.ai_agent_connected";
const FAILED: &str = "settings.feedback.ai_agent_test_failed";
const HTTP_FIXTURE: &str =
    "ai::settings_ui::list_connection_test::tests::saved_list_probe_http_event_loop_fixture";

fn profile(id: &str) -> AiAgentProfile {
    AiAgentProfile {
        id: id.into(),
        name: format!("Saved {id}"),
        connection: AiAgentSettings {
            base_url: "http://127.0.0.1:1".into(),
            model: "saved-model".into(),
            headers: vec![
                AiAgentHeader {
                    name: "X-Fixture-Auth".into(),
                    value: SECRET.into(),
                    enabled: true,
                },
                AiAgentHeader {
                    name: "X-Disabled".into(),
                    value: "must-not-send".into(),
                    enabled: false,
                },
            ],
            ..Default::default()
        },
    }
}

fn fixture() -> (MainWindow, GitHubSettingsPage, Binding) {
    let window = MainWindow::new().unwrap();
    crate::i18n::initialize(&window, "en").unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    page.set_window_width(1200.0);
    page.set_window_height(900.0);
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    let binding = Binding {
        page: page.as_weak(),
        facade: AiAgentSettingsFacade::memory(AiAgentConfiguration {
            active_id: "a".into(),
            profiles: vec![profile("a"), profile("b")],
        }),
        probe: Arc::default(),
    };
    (window, page, binding)
}

fn activate_list(page: &GitHubSettingsPage) {
    let label = crate::i18n::tr("settings.ai_agent.title");
    let navigation = ElementHandle::find_by_element_type_name(page, "GitHubSettingsNavItem")
        .find(|item| item.accessible_label().as_deref() == Some(label.as_ref()))
        .expect("AI Agent navigation");
    // Use real dispatch without advancing the mock clock: the dedicated HTTP
    // fixture runs with system time so event-loop timers can enforce a deadline.
    let p = navigation.absolute_position();
    let size = navigation.size();
    let position = slint::LogicalPosition::new(p.x + size.width / 2.0, p.y + size.height / 2.0);
    page.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    page.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    page.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    assert!(page.get_ai_agent_list_active());
}

fn request(binding: &Binding, page: &GitHubSettingsPage, id: &str) -> Request {
    let state = AppState::get(page);
    state.set_ai_agent_list_feedback_open(false);
    state.set_ai_agent_list_testing_id(id.into());
    Request {
        ticket: binding.probe.next_ticket(),
        profile: binding
            .facade
            .snapshot()
            .state()
            .profile(id)
            .unwrap()
            .clone(),
    }
}

#[test]
fn saved_profile_completion_survives_peer_activation_and_refreshes_dismissed_locale_copy() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = fixture();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
    activate_list(&page);
    let captured = request(&binding, &page, "a");
    binding
        .facade
        .activate_profile("b")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .applied
        .unwrap();
    let state = AppState::get(&page);
    state.set_ai_agent_editor_model("unsaved-editor-model".into());
    state.set_ai_agent_editor_api_key("unsaved-editor-secret".into());
    binding.complete(captured, true, CONNECTED);
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    assert!(state.get_ai_agent_list_feedback_open());
    assert!(state.get_ai_agent_list_feedback_success());
    assert_eq!(state.get_ai_agent_list_feedback_key(), CONNECTED);
    let english = state.get_ai_agent_list_feedback();
    crate::i18n::apply_preference("zh-CN").unwrap();
    super::super::sync_language(&page);
    assert_ne!(state.get_ai_agent_list_feedback(), english);
    assert_eq!(
        state.get_ai_agent_list_feedback(),
        crate::i18n::tr(CONNECTED).as_ref()
    );
    state.set_ai_agent_list_feedback_open(false);
    crate::i18n::apply_preference("en").unwrap();
    super::super::sync_language(&page);
    assert_eq!(state.get_ai_agent_list_feedback(), english);
    assert!(
        !state.get_ai_agent_list_feedback_open(),
        "locale refresh never reopens a dismissed Toast"
    );
    assert!(!english.contains(SECRET));
    assert_eq!(binding.facade.snapshot().state().active_id, "b");
    assert_eq!(
        binding.facade.snapshot().state().profile("a"),
        Some(&profile("a"))
    );
    assert_eq!(state.get_ai_agent_editor_model(), "unsaved-editor-model");
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn changed_deleted_or_hidden_profile_completion_clears_busy_without_feedback() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for change in ["name", "model", "header", "delete", "route", "editor"] {
        let (_window, page, binding) = fixture();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
        activate_list(&page);
        let captured = request(&binding, &page, "b");
        match change {
            "delete" => {
                binding
                    .facade
                    .delete_profile("b")
                    .unwrap()
                    .unwrap()
                    .wait()
                    .unwrap()
                    .applied
                    .unwrap();
            }
            "route" => page.set_route_active(false),
            "editor" => AppState::get(&page).set_ai_agent_editor_open(true),
            _ => {
                let mut updated = profile("b");
                match change {
                    "name" => updated.name = "Renamed".into(),
                    "model" => updated.connection.model = "changed-model".into(),
                    _ => updated.connection.headers[0].value = "changed-secret".into(),
                }
                binding
                    .facade
                    .update_profile(updated)
                    .unwrap()
                    .unwrap()
                    .wait()
                    .unwrap()
                    .applied
                    .unwrap();
            }
        }
        binding.complete(captured, true, CONNECTED);
        let state = AppState::get(&page);
        assert!(state.get_ai_agent_list_testing_id().is_empty(), "{change}");
        assert!(!state.get_ai_agent_list_feedback_open(), "{change}");
        assert!(
            state.get_ai_agent_list_feedback_key().is_empty(),
            "{change}"
        );
        binding.facade.shutdown();
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn cancelled_late_result_cannot_clear_a_new_probe_or_replace_its_failure() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = fixture();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
    activate_list(&page);
    let old = request(&binding, &page, "a");
    binding.cancel();
    let current = request(&binding, &page, "b");
    binding.complete(old, true, CONNECTED);
    let state = AppState::get(&page);
    assert_eq!(state.get_ai_agent_list_testing_id(), "b");
    assert!(!state.get_ai_agent_list_feedback_open());
    binding.complete(current, false, FAILED);
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    assert!(state.get_ai_agent_list_feedback_open());
    assert!(!state.get_ai_agent_list_feedback_success());
    assert_eq!(state.get_ai_agent_list_feedback_key(), FAILED);
    assert_eq!(
        state.get_ai_agent_list_feedback(),
        crate::i18n::tr(FAILED).as_ref()
    );
    binding.cancel();
    assert!(!state.get_ai_agent_list_feedback_open());
    assert!(state.get_ai_agent_list_feedback().is_empty());
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn list_probe_rejects_unknown_inactive_and_duplicate_requests_before_networking() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = fixture();
    bind(&page, binding.facade.clone());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
    let state = AppState::get(&page);
    assert!(!page.get_ai_agent_list_active());
    state.invoke_test_ai_agent("a".into());
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    activate_list(&page);
    state.invoke_test_ai_agent("missing".into());
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    state.set_ai_agent_list_testing_id("a".into());
    state.invoke_test_ai_agent("b".into());
    assert_eq!(state.get_ai_agent_list_testing_id(), "a");
    state.invoke_cancel_ai_agent_list_test();
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn shared_probe_cancel_and_owner_drop_abort_the_real_owned_task() {
    for explicit_cancel in [true, false] {
        let probe = Arc::new(Probe::default());
        let ticket = probe.next_ticket();
        let (entered, received) = std::sync::mpsc::channel();
        let task = crate::app_runtime::shared_runtime().spawn(async move {
            entered.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        probe.attach(task.abort_handle());
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        if explicit_cancel {
            probe.cancel();
            assert!(!probe.finish(ticket), "cancelled result loses its ticket");
            let next = probe.next_ticket();
            assert!(probe.finish(next));
            assert!(!probe.finish(next), "a completed ticket is consumed once");
        }
        drop(probe);
        let error = crate::app_runtime::shared_runtime()
            .block_on(task)
            .unwrap_err();
        assert!(error.is_cancelled(), "cancel={explicit_cancel}");
    }
}

#[test]
fn saved_list_probe_uses_real_shared_http_and_localized_results() {
    for locale in ["en", "zh-CN"] {
        for response in ["listed", "unlisted", "failed"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", HTTP_FIXTURE, "--nocapture"])
                .env("USHER_LIST_PROBE_TEST_LOCALE", locale)
                .env("USHER_LIST_PROBE_TEST_RESPONSE", response)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{locale}/{response}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
#[ignore = "isolated event-loop process invoked by the shared HTTP regression"]
fn saved_list_probe_http_event_loop_fixture() {
    use std::io::{Read, Write};
    use std::sync::mpsc;
    i_slint_backend_testing::init_integration_test_with_system_time();
    let locale = std::env::var("USHER_LIST_PROBE_TEST_LOCALE").unwrap();
    let response = std::env::var("USHER_LIST_PROBE_TEST_RESPONSE").unwrap();
    let (_window, page, binding) = fixture();
    crate::i18n::apply_preference(&locale).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    slint::platform::update_timers_and_animations();
    activate_list(&page);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut saved = profile("a");
    saved.connection.base_url = format!("http://{}", listener.local_addr().unwrap());
    binding
        .facade
        .update_profile(saved.clone())
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .applied
        .unwrap();
    bind(&page, binding.facade.clone());
    let (seen, received) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let body = match response.as_str() {
        "listed" => r#"{"data":[{"id":"saved-model"}]}"#,
        "unlisted" => r#"{"data":[]}"#,
        _ => SECRET,
    };
    let status = if response == "failed" {
        "401 Unauthorized"
    } else {
        "200 OK"
    };
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 2048];
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0 && request.len() + count <= 16 * 1024);
            request.extend_from_slice(&buffer[..count]);
        }
        seen.send(String::from_utf8(request).unwrap()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let _ = stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
    });
    let state = AppState::get(&page);
    state.set_ai_agent_editor_base_url("http://127.0.0.1:1/editor-draft".into());
    state.set_ai_agent_editor_api_key("draft-key-never-send".into());
    state.set_ai_agent_editor_model("draft-model".into());
    state.invoke_test_ai_agent("a".into());
    assert_eq!(
        state.get_ai_agent_list_testing_id(),
        "a",
        "admission returns before response"
    );
    state.invoke_test_ai_agent("b".into());
    assert_eq!(
        state.get_ai_agent_list_testing_id(),
        "a",
        "single list probe guards duplicate starts"
    );
    let request = received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .to_ascii_lowercase();
    assert!(request.starts_with("get /models "));
    assert!(request.contains(&format!("x-fixture-auth: {SECRET}\r\n")));
    assert!(!request.contains("x-disabled:"));
    assert!(!request.contains("authorization:"));
    assert!(!request.contains("draft-key-never-send"));
    binding
        .facade
        .activate_profile("b")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .applied
        .unwrap();
    release.send(()).unwrap();
    let timer = slint::Timer::default();
    let weak = page.as_weak();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(5),
        move || {
            if weak
                .upgrade()
                .is_some_and(|page| AppState::get(&page).get_ai_agent_list_feedback_open())
                || std::time::Instant::now() > deadline
            {
                slint::quit_event_loop().unwrap();
            }
        },
    );
    slint::run_event_loop().unwrap();
    server.join().unwrap();
    assert!(state.get_ai_agent_list_testing_id().is_empty());
    assert!(
        state.get_ai_agent_list_feedback_open(),
        "real request completed on the Slint event loop"
    );
    assert_eq!(
        state.get_ai_agent_list_feedback_success(),
        response != "failed"
    );
    let key = match response.as_str() {
        "listed" => CONNECTED,
        "unlisted" => "settings.feedback.ai_agent_connected_model_unlisted",
        _ => FAILED,
    };
    assert_eq!(state.get_ai_agent_list_feedback_key(), key);
    assert_eq!(
        state.get_ai_agent_list_feedback(),
        usher_i18n::translate_for(&locale, key).as_ref()
    );
    assert!(!state.get_ai_agent_list_feedback().contains(SECRET));
    assert_eq!(state.get_ai_agent_editor_model(), "draft-model");
    assert_eq!(binding.facade.snapshot().state().profile("a"), Some(&saved));
    assert_eq!(binding.facade.snapshot().state().active_id, "b");
    binding.facade.shutdown();
}
