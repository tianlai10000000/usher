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
fn mode_switch_mounts_latest_history_before_revealing_viewport() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ai = JsonlHistoryStore::open_test_path(directory.path().join("ai.jsonl")).unwrap();
    let rag = JsonlHistoryStore::open_test_path(directory.path().join("rag.jsonl")).unwrap();
    for (store, count) in [(&ai, 2), (&rag, 100)] {
        for id in 1..=count {
            store
                .append(&StoredChatMessage {
                    agent: None,
                    id,
                    role: if id % 2 == 0 {
                        StoredMessageRole::Assistant
                    } else {
                        StoredMessageRole::User
                    },
                    status: StoredMessageStatus::Normal,
                    content: format!("Mode {count} message {id}"),
                    timestamp: "09:31".into(),
                    elapsed_ms: None,
                })
                .unwrap();
        }
    }
    let ui = MainWindow::new().unwrap();
    ui.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(800.0, 600.0),
    });
    ui.set_platform_is_macos(false);
    ui.set_show_navigation_drawer(false);
    ui.set_show_breadcrumb(false);
    let mut sessions = ChatSessions::from_test_stores(ai, rag);
    // Re-enter a history that has already been paged beyond the mounted-row cap.
    while sessions.can_load_more_for(ChatMode::Rag) {
        let request = sessions.prepare_history_load_for(ChatMode::Rag).unwrap();
        let page = request.load().unwrap();
        sessions
            .commit_history_load_for(ChatMode::Rag, &request, page)
            .unwrap();
    }
    setup_interactive_with_sessions(&ui, Some(sessions));
    ui.set_current_route(Route::Chat);
    ui.set_show_chat_page(true);
    ui.invoke_chat_route_visible_changed(true);
    frames(&ui, 80);
    for _ in 0..3 {
        for (ai_mode, last_id) in [(false, 100), (true, 2)] {
            ui.invoke_chat_switch_mode(ai_mode);
            // Verify the first revealed frame as well as the settled result;
            // waiting for later scroll callbacks masks an initially blank tail.
            for frame_count in [1, 80] {
                frames(&ui, frame_count);
                assert!(!ui.get_chat_history_restoring());
                let offset = -ui.get_chat_scroll_position();
                let latest_visible = MESSAGE_GEOMETRIES.with(|g| {
                    g.borrow().get(&last_id).is_some_and(|row| {
                        row.y + row.height > offset
                            && row.y < offset + ui.get_chat_message_viewport_height()
                    })
                });
                if !latest_visible {
                    snapshot(&ui, "mode switch missing latest message");
                }
                assert!(
                    latest_visible,
                    "mode {ai_mode}: latest message must be visible without wheel input"
                );
                assert!(ui.get_chat_messages().row_count() <= 32);
                assert_eq!(ui.get_chat_virtual_bottom_spacer(), 0.0);
                if ai_mode {
                    assert!(
                        offset.abs() <= 0.5,
                        "short history must fit without scrolling"
                    );
                }
            }
        }
    }
}

#[test]
fn real_layout_prepend_and_bidirectional_scrolling() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let ai = JsonlHistoryStore::open_test_path(directory.path().join("ai.jsonl")).unwrap();
    let rag = JsonlHistoryStore::open_test_path(directory.path().join("rag.jsonl")).unwrap();
    for id in 1..=100 {
        let message = StoredChatMessage {
            agent: None,
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
        let viewport =
            ElementHandle::find_by_element_id(&ui, "GitHubChatPage::message-scroll-view")
                .next()
                .expect("message viewport");
        let pointer = slint::LogicalPosition::new(
            viewport.absolute_position().x + viewport.size().width / 2.0,
            viewport.absolute_position().y + viewport.size().height / 2.0,
        );
        // Estimated document height is not the Flickable's actual scroll limit.
        // Wheel events exercise production clamping, cancellation and callbacks.
        for delta_y in [48.0, -48.0, 48.0] {
            let mut reached_edge = false;
            for _ in 0..2048 {
                ui.window().dispatch_event(WindowEvent::PointerScrolled {
                    position: pointer,
                    delta_x: 0.0,
                    delta_y,
                });
                frames(&ui, 4);
                let offset = -ui.get_chat_scroll_position();
                assert!(
                    ui.get_chat_messages().row_count() > 0,
                    "scrolling must not empty the message model"
                );
                let intersects = MESSAGE_GEOMETRIES.with(|g| {
                    g.borrow().values().any(|row| {
                        row.y + row.height > -ui.get_chat_scroll_position()
                            && row.y
                                < -ui.get_chat_scroll_position()
                                    + ui.get_chat_message_viewport_height()
                    })
                });
                if !intersects {
                    snapshot(&ui, "missing viewport coverage");
                }
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
                reached_edge = if delta_y > 0.0 {
                    offset <= 0.5
                } else {
                    ui.get_chat_bottom_distance() <= 0.5
                };
                if reached_edge {
                    let edge_id = if delta_y > 0.0 { 1 } else { 100 };
                    assert!(
                        MESSAGE_GEOMETRIES.with(|g| g
                            .borrow()
                            .get(&edge_id)
                            .is_some_and(|row| row.y + row.height > offset
                                && row.y < offset + ui.get_chat_message_viewport_height())),
                        "{mode:?}: boundary message {edge_id} must be visible"
                    );
                    break;
                }
            }
            assert!(
                reached_edge,
                "{mode:?}: scrolling must reach the document edge"
            );
        }
    }
}
