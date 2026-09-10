use super::*;
use slint::platform::WindowEvent;
use std::time::Duration;
use usher_messages::{MessageDraft, NoopDesktopNotifier};

#[test]
fn pagination_links_are_bounded_ordered_and_include_current_and_endpoints() {
    for total in 1..=50 {
        for current in 1..=total {
            let links = pagination_links(current, total);
            assert!(links.len() <= 9, "{current}/{total}: {links:?}");
            assert_eq!(links[0], 1);
            assert_eq!(links.last(), Some(&(total as i32)));
            assert!(links.contains(&(current as i32)));
            let pages: Vec<_> = links.iter().copied().filter(|page| *page != 0).collect();
            assert!(pages.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(!links.windows(2).any(|pair| pair == [0, 0]));
        }
    }
    assert_eq!(pagination_links(1, 50), [1, 2, 3, 4, 5, 6, 7, 0, 50]);
    assert_eq!(pagination_links(25, 50), [1, 0, 23, 24, 25, 26, 27, 0, 50]);
    assert_eq!(pagination_links(50, 50), [1, 0, 44, 45, 46, 47, 48, 49, 50]);
}

fn settle() {
    for _ in 0..4 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

fn assert_visible_rows(page: &GitHubMessagesPage, expected: usize) {
    assert_eq!(page.get_page_row_count() as usize, expected);
    assert_eq!(page.get_messages().row_count(), MESSAGE_PAGE_SIZE);
    for row in page.get_messages().iter().skip(expected) {
        assert!(message_row_equivalent(&row, &MessageListItem::default()));
    }
}

#[test]
fn compiled_inbox_pages_filters_and_reentry_keep_only_twenty_rows() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    for index in 0..41 {
        center
            .publish(MessageDraft::new(format!("Message {index}"), "Fixture").desktop(false))
            .unwrap();
    }
    let window = MainWindow::new().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1_000.0, 700.0),
    });
    let bridge = MessagePageBridge::new(center.clone());
    bridge.install(&window);
    window.set_current_route(crate::Route::Messages);
    settle();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    assert_eq!(page.get_total_count(), 41);
    assert_eq!(page.get_unread_count(), 41);
    assert_eq!(page.get_page_count(), 3);
    assert_eq!(page.get_messages().row_count(), 20);

    let first_ids: Vec<_> = page.get_messages().iter().map(|row| row.id).collect();
    let model = page.get_messages();
    page.invoke_set_read(first_ids[0].clone(), true);
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_unread_count(), 40);
    assert_eq!(
        page.get_messages(),
        model,
        "read changes retain model identity"
    );
    assert_eq!(
        page.get_messages()
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        first_ids
    );

    page.set_scroll_y(-48.0);
    for filter in [1, 0, 1, 0] {
        page.invoke_change_filter(filter);
        settle();
        assert_eq!(page.get_scroll_y(), -48.0, "filter tabs preserve viewport");
        assert_eq!(page.get_current_page(), 1);
    }

    page.set_scroll_y(-500.0);
    page.invoke_change_page(3);
    assert_eq!(page.get_current_page(), 3);
    assert_eq!(page.get_scroll_y(), 0.0);
    assert_visible_rows(&page, 1);
    assert_eq!(
        page.get_messages(),
        model,
        "page changes preserve delegate slots"
    );
    assert_eq!(
        page.get_messages().row_data(0).unwrap().id.as_str(),
        center.snapshot().messages[40].id
    );
    let last_id = page.get_messages().row_data(0).unwrap().id;
    page.invoke_delete_message(last_id);
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_current_page(), 2);
    assert_eq!(page.get_page_count(), 2);
    assert_eq!(page.get_messages().row_count(), 20);

    page.set_scroll_y(-500.0);
    page.invoke_change_filter(2);
    settle();
    assert_eq!(
        page.get_scroll_y(),
        0.0,
        "short results clamp an out-of-range viewport"
    );
    assert_eq!(page.get_selected_filter(), 2);
    assert_eq!(page.get_current_page(), 1);
    assert_eq!(page.get_page_count(), 1);
    assert_visible_rows(&page, 1);
    assert!(page.get_messages().row_data(0).unwrap().read);
    page.invoke_change_filter(1);
    assert_eq!(page.get_unread_count(), 39);
    assert_eq!(page.get_messages().row_count(), 20);
    assert!(page.get_messages().iter().all(|row| !row.read));

    bridge.set_active(false);
    assert_visible_rows(&page, 0);
    assert_eq!(page.get_messages(), model);
    bridge.sync_language("en");
    assert_visible_rows(&page, 0);
    center
        .publish(MessageDraft::new("Off-page update", "Fixture").desktop(false))
        .unwrap();
    bridge.set_active(true);
    assert_eq!(page.get_messages(), model);
    assert_eq!(page.get_messages().row_count(), 20);
    assert_eq!(page.get_unread_count(), 40);
    assert_eq!(
        page.get_messages().row_data(0).unwrap().title,
        "Off-page update"
    );

    page.invoke_mark_all_read();
    apply_page_snapshot(&page, center.snapshot());
    assert_visible_rows(&page, 0);
    assert_eq!(page.get_page_count(), 1);
    assert_eq!(page.get_unread_count(), 0);
    page.invoke_change_filter(0);
    page.invoke_clear_messages();
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_total_count(), 0);
    assert_visible_rows(&page, 0);
    center
        .publish(MessageDraft::new("After clear", "Fixture").desktop(false))
        .unwrap();
    apply_page_snapshot(&page, center.snapshot());
    assert_visible_rows(&page, 1);
    assert_eq!(page.get_messages(), model);
    assert_eq!(
        page.get_messages().row_data(0).unwrap().title,
        "After clear"
    );
    center.shutdown();
}
