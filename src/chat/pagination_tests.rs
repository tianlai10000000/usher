//! Exercise the production model bridge, geometry callbacks and scroll scheduler
//! against the compiled root UI, not just the anchor arithmetic.
use super::*;
use crate::chat::persistence::{JsonlHistoryStore, StoredChatMessage};
use i_slint_backend_testing::ElementHandle;
use slint::platform::WindowEvent;
use std::time::Duration;

fn settle(ui: &MainWindow) {
    frames(ui, 24);
}

fn frames(ui: &MainWindow, count: usize) {
    let mut list =
        ElementHandle::find_by_element_id(ui, "GitHubChatPage::mounted-message-list").next();
    for _ in 0..count {
        if let Some(list) = &list {
            let _ = (list.absolute_position(), list.size());
        }
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
        if list.is_none() {
            // Element queries omit an offscreen list during a random-access
            // jump, before the queued virtual-window commit brings it back.
            list = ElementHandle::find_by_element_id(ui, "GitHubChatPage::mounted-message-list")
                .next();
        }
    }
}

fn snapshot(ui: &MainWindow, label: &str) {
    let signature = LAST_RENDER_SIGNATURE.with(|s| *s.borrow());
    let geometry = MESSAGE_GEOMETRIES.with(|g| {
        let mut rows: Vec<_> = g
            .borrow()
            .iter()
            .map(|(id, g)| (*id, g.y, g.height))
            .collect();
        rows.sort_by_key(|row| row.0);
        rows
    });
    eprintln!(
        "{label}: offset={} viewport={} signature={signature:?} rows={geometry:?}",
        -ui.get_chat_scroll_position(),
        ui.get_chat_message_viewport_height()
    );
}

#[test]
fn real_layout_prepend_and_bidirectional_scrolling() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ai = JsonlHistoryStore::open_test_path(directory.path().join("ai.jsonl")).unwrap();
    let rag = JsonlHistoryStore::open_test_path(directory.path().join("rag.jsonl")).unwrap();
    for id in 1..=100 {
        let message = StoredChatMessage {
            id,
            role: if id % 3 == 0 {
                StoredMessageRole::Assistant
            } else {
                StoredMessageRole::User
            },
            status: StoredMessageStatus::Normal,
            content: if id % 13 == 0 {
                format!("LONG-{id} {}", "长消息布局测试。".repeat(160))
            } else {
                format!("MESSAGE-{id}")
            },
            timestamp: "09:31".into(),
            elapsed_ms: None,
        };
        ai.append(&message).unwrap();
        rag.append(&message).unwrap();
    }
    let ui = MainWindow::new().unwrap();
    ui.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(800.0, 600.0),
    });
    ui.set_platform_is_macos(false);
    ui.set_show_navigation_drawer(false);
    ui.set_show_breadcrumb(false);
    setup_interactive_with_sessions(&ui, Some(ChatSessions::from_test_stores(ai, rag)));
    ui.set_current_route(Route::Chat);
    ui.set_show_chat_page(true);
    ui.set_chat_history_restoring(false);
    let session = CHAT_SESSION.with(|slot| slot.borrow().clone().unwrap());
    // Synchronous fixture I/O replaces only transport. Commit and all subsequent
    // layout/anchor/scroll work use the same production code as a completed page.
    let weak = ui.as_weak();
    let loading_session = session.clone();
    ui.on_chat_load_more_history(move || {
        let ui = weak.upgrade().unwrap();
        let mode = chat_mode_from_ui(&ui);
        let mut guard = loading_session.lock().unwrap();
        let sessions = guard.as_mut().unwrap();
        let request = sessions.prepare_history_load_for(mode).unwrap();
        let page = request.load().unwrap();
        sessions
            .commit_history_load_for(mode, &request, page)
            .unwrap();
        drop(guard);
        capture_visible_history_anchor(&ui);
        apply_chat_state_to_ui(&ui, &loading_session, mode, false, None, false);
    });
    for mode in [ChatMode::Ai, ChatMode::Rag] {
        clear_history_anchor_state();
        clear_virtual_layout_cache();
        LAST_RENDER_SIGNATURE.with(|last| *last.borrow_mut() = None);
        ui.set_chat_ai_mode(mode == ChatMode::Ai);
        apply_chat_state_to_ui(&ui, &session, mode, false, None, false);
        settle(&ui);
        settle(&ui);
        settle(&ui);
        ui.invoke_chat_user_scroll_started();
        ui.set_chat_scroll_position(0.0);
        settle(&ui);
        // The top transition may itself load a page. Reach the newly exposed
        // history boundary before explicitly activating the action below.
        ui.invoke_chat_user_scroll_started();
        ui.set_chat_scroll_position(0.0);
        settle(&ui);
        snapshot(&ui, "before prepend");
        let old_first = session.lock().unwrap().as_ref().unwrap().messages_for(mode)[0].id as i32;
        let old_y =
            MESSAGE_GEOMETRIES.with(|g| g.borrow()[&old_first].y) + ui.get_chat_scroll_position();
        ui.invoke_chat_load_more_history();
        settle(&ui);
        snapshot(&ui, "after prepend");
        let new_y =
            MESSAGE_GEOMETRIES.with(|g| g.borrow()[&old_first].y) + ui.get_chat_scroll_position();
        assert!(
            (new_y - old_y).abs() < 0.5,
            "old reading anchor moved {old_y} -> {new_y}"
        );
        let newest_older = old_first - 1;
        let geometry = MESSAGE_GEOMETRIES.with(|g| g.borrow().get(&newest_older).copied());
        assert!(
            geometry.is_some(),
            "newest older message must be mounted, not hidden in the top spacer"
        );
        let geometry = geometry.unwrap();
        let offset = -ui.get_chat_scroll_position();
        assert!(
            geometry.y + geometry.height > offset
                && geometry.y < offset + ui.get_chat_message_viewport_height(),
            "newest older message must intersect the viewport"
        );
        // Load the remaining history and sweep both ways in small wheel-sized
        // steps, including same-bucket reversals immediately after a prepend.
        while ui.get_chat_can_load_more_history() {
            ui.invoke_chat_load_more_history();
            settle(&ui);
        }
        snapshot(&ui, "all history loaded");
        let maximum = current_virtual_document_height_px() - ui.get_chat_message_viewport_height();
        let positions: Vec<f32> = (0..=(maximum / 48.0) as usize)
            .map(|step| step as f32 * 48.0)
            .collect();
        for offset in positions.iter().chain(positions.iter().rev()) {
            ui.invoke_chat_user_scroll_started();
            ui.set_chat_scroll_position(-offset);
            ui.invoke_chat_scroll_position_changed(-offset);
            frames(&ui, 4);
            assert!(
                ui.get_chat_messages().row_count() > 0,
                "scrolling must not empty the message model"
            );
            let intersects = MESSAGE_GEOMETRIES.with(|g| {
                g.borrow().values().any(|row| {
                    row.y + row.height > -ui.get_chat_scroll_position()
                        && row.y
                            < -ui.get_chat_scroll_position() + ui.get_chat_message_viewport_height()
                })
            });
            assert!(
                intersects,
                "{mode:?}: viewport must contain a real message at {offset}"
            );
            let blank = virtual_blank_exposure_px(
                -ui.get_chat_scroll_position(),
                ui.get_chat_message_viewport_height(),
            );
            if blank > 1.0 {
                snapshot(&ui, "blank");
            }
            assert!(
                blank <= 1.0,
                "{mode:?}: offset {offset} exposes {blank}px virtual blank"
            );
            assert!(ui.get_chat_messages().row_count() <= 32);
        }
    }
}
