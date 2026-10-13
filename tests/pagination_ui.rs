use std::{cell::RefCell, rc::Rc, time::Duration};

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model, ModelRc, VecModel};
use usher_lib::{MainWindow, Route, WorkflowPullRequestItem};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(250));
}

fn find(window: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing UI element {id}"))
}

fn pagination(window: &MainWindow) -> ElementHandle {
    ElementHandle::find_by_element_type_name(window, "GitHubPagination")
        .next()
        .expect("PR pagination")
}

fn link(window: &MainWindow, label: &str) -> ElementHandle {
    pagination(window)
        .query_descendants()
        .match_inherits("GitHubPaginationLink")
        .find_all()
        .into_iter()
        .find(|element| element.accessible_label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("missing pagination link {label}"))
}

fn page_links(window: &MainWindow) -> Vec<ElementHandle> {
    pagination(window)
        .query_descendants()
        .match_id("GitHubPagination::page-link")
        .find_all()
}

fn resize(window: &MainWindow, width: f32) {
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(width, 1_200.0),
    });
    settle();
}

fn project_page(window: &MainWindow, current: i32, total: i32) {
    window.set_workflow_pr_list_page(current);
    window.set_workflow_pr_list_page_count(total);
    window.set_workflow_pr_list_page_links(ModelRc::new(VecModel::from(
        usher_ui::pagination_links(current as usize, total as usize),
    )));
    settle();
}

fn fixture() -> MainWindow {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    slint::select_bundled_translation("en").unwrap();
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::Workflow);
    window.set_workflow_selected_flow(2);
    window.set_workflow_pr_repo_type("github".into());
    window.set_workflow_pr_list_items(ModelRc::new(VecModel::from(vec![
        WorkflowPullRequestItem {
            number: "190".into(),
            title: "Pagination fixture".into(),
            state: "open".into(),
            author: "fixture".into(),
            ..Default::default()
        },
    ])));
    resize(&window, 1_280.0);
    project_page(&window, 2, 20);
    window
}

fn assert_centered_outside_card(window: &MainWindow) {
    let card = find(window, "GitHubPullRequestFlow::pr-list");
    let pagination = pagination(window);
    assert!(
        (pagination.absolute_position().y - card.absolute_position().y - card.size().height - 16.0)
            .abs()
            < 0.5,
        "pagination must remain 16 px below the collection border"
    );
    let previous = find(window, "GitHubPagination::previous-link");
    let next = find(window, "GitHubPagination::next-link");
    let left = previous.absolute_position().x;
    let right = next.absolute_position().x + next.size().width;
    assert!(left >= pagination.absolute_position().x);
    assert!(right <= pagination.absolute_position().x + pagination.size().width + 0.5);
    assert!(
        ((left + right) / 2.0 - pagination.absolute_position().x - pagination.size().width / 2.0)
            .abs()
            < 0.5,
        "the visible navigation links must stay centered"
    );
}

#[test]
fn pr_pagination_navigates_numbers_and_directions_and_refreshes_mounted_locale() {
    let window = fixture();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let selected_callback = selected.clone();
    let weak = window.as_weak();
    window.on_workflow_pr_list_page_change(move |page| {
        selected_callback.borrow_mut().push(page);
        let window = weak.unwrap();
        window.set_workflow_pr_list_page(page);
        if window.get_workflow_pr_list_page_links().row_count() > 0 {
            window.set_workflow_pr_list_page_links(ModelRc::new(VecModel::from(
                usher_ui::pagination_links(
                    page as usize,
                    window.get_workflow_pr_list_page_count() as usize,
                ),
            )));
        }
    });

    assert_centered_outside_card(&window);
    assert_eq!(page_links(&window).len(), 9);
    assert_eq!(link(&window, "Page 2").accessible_checked(), Some(true));
    assert_eq!(link(&window, "Page 20").accessible_checked(), Some(false));
    link(&window, "Page 20").mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20]);
    assert_eq!(link(&window, "Page 20").accessible_checked(), Some(true));
    let next = link(&window, "Next");
    assert_eq!(next.accessible_enabled(), Some(false));
    next.mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20]);
    link(&window, "Previous").mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20, 19]);
    link(&window, "Page 1").mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20, 19, 1]);
    let previous = link(&window, "Previous");
    assert_eq!(previous.accessible_enabled(), Some(false));
    previous.mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20, 19, 1]);

    // Truncation indicators occupy their slot, but have no actionable page.
    let ellipsis = page_links(&window)
        .into_iter()
        .find(|element| element.accessible_label().as_deref() == Some(""))
        .expect("noninteractive truncation indicator");
    assert_eq!(ellipsis.accessible_enabled(), Some(false));
    ellipsis.mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20, 19, 1]);

    // The same mounted links repaint after switching the runtime translation.
    let previous = link(&window, "Previous");
    let next = link(&window, "Next");
    let current = link(&window, "Page 1");
    slint::select_bundled_translation("zh-CN").unwrap();
    settle();
    assert_eq!(previous.accessible_label().as_deref(), Some("上一页"));
    assert_eq!(next.accessible_label().as_deref(), Some("下一页"));
    assert_eq!(current.accessible_label().as_deref(), Some("页码 1"));
    next.mock_single_click(PointerEventButton::Left);
    assert_eq!(*selected.borrow(), [20, 19, 1, 2]);
    assert_eq!(link(&window, "页码 2").accessible_checked(), Some(true));
    // Directional links also remain operable with the keyboard after focus.
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Return.into(),
    });
    assert_eq!(*selected.borrow(), [20, 19, 1, 2, 3]);
    slint::select_bundled_translation("en").unwrap();
    settle();
    assert_eq!(previous.accessible_label().as_deref(), Some("Previous"));
    assert_eq!(next.accessible_label().as_deref(), Some("Next"));
    assert_centered_outside_card(&window);
    assert_fallbacks(&window, &selected);
    assert_hidden_states(&window);
}

fn assert_fallbacks(window: &MainWindow, selected: &RefCell<Vec<i32>>) {
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        resize(window, 1_280.0);
        project_page(window, 10, 20);
        assert_eq!(page_links(window).len(), 9);
        assert_centered_outside_card(window);
        resize(window, 480.0);
        assert!(page_links(window).is_empty());
        assert_centered_outside_card(window);

        // A large confirmed count must include the actual width of its digits.
        resize(window, 1_800.0);
        project_page(window, 1_000_000_000, i32::MAX);
        let links = page_links(window);
        assert_eq!(links.len(), 9);
        for pair in links.windows(2) {
            assert!(
                pair[0].absolute_position().x + pair[0].size().width
                    <= pair[1].absolute_position().x + 0.5,
                "multi-digit page links must not overlap"
            );
        }
        assert_centered_outside_card(window);
        resize(window, 720.0);
        assert!(page_links(window).is_empty());
        assert_centered_outside_card(window);

        resize(window, 1_280.0);
        window.set_workflow_pr_list_page(2);
        window.set_workflow_pr_list_page_count(3);
        window.set_workflow_pr_list_page_links(ModelRc::new(VecModel::from(Vec::<i32>::new())));
        settle();
        assert!(page_links(window).is_empty());
        assert_eq!(
            find(window, "GitHubPagination::previous-link").accessible_enabled(),
            Some(true)
        );
        assert_eq!(
            find(window, "GitHubPagination::next-link").accessible_enabled(),
            Some(true)
        );
        assert_centered_outside_card(window);
        let before = selected.borrow().len();
        find(window, "GitHubPagination::next-link").mock_single_click(PointerEventButton::Left);
        assert_eq!(selected.borrow().len(), before + 1);
        assert_eq!(selected.borrow().last(), Some(&3));
        assert!(page_links(window).is_empty());
    }
}

fn assert_hidden_states(window: &MainWindow) {
    window.set_workflow_pr_list_loading(true);
    settle();
    assert!(
        ElementHandle::find_by_element_type_name(window, "GitHubPagination")
            .next()
            .is_none()
    );
    window.set_workflow_pr_list_loading(false);
    window.set_workflow_pr_list_error("Fixture error".into());
    settle();
    assert!(
        ElementHandle::find_by_element_type_name(window, "GitHubPagination")
            .next()
            .is_none()
    );
    window.set_workflow_pr_list_error("".into());
    project_page(window, 1, 1);
    assert!(
        ElementHandle::find_by_element_type_name(window, "GitHubPagination")
            .next()
            .is_none()
    );
    project_page(window, 1, 20);
    window.set_workflow_pr_list_items(ModelRc::new(VecModel::from(
        Vec::<WorkflowPullRequestItem>::new(),
    )));
    settle();
    assert!(
        ElementHandle::find_by_element_type_name(window, "GitHubPagination")
            .next()
            .is_none()
    );
}
