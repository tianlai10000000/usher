#![cfg(feature = "ui-components")]

#[path = "support/ui_components.rs"]
mod playground;

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route, install_transient_input_bridge};

#[test]
fn showcase_action_menu_anchors_floats_and_toggles() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    playground::install(&window);
    install_transient_input_bridge(&window).expect("transient input bridge");
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    window.window().set_size(LogicalSize::new(1000.0, 800.0));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let find = |id| {
        ElementHandle::find_by_element_id(&window, id)
            .next()
            .unwrap_or_else(|| panic!("missing UI element {id}"))
    };
    for (locale, width, labels, back) in [
        (
            "en",
            1000.0,
            [
                "Foundations",
                "Actions",
                "Containment",
                "Communication",
                "Navigation",
                "Selection",
                "Text input",
                "Scenarios",
            ],
            "Back to top",
        ),
        (
            "zh-CN",
            600.0,
            [
                "基础",
                "操作",
                "容器",
                "反馈",
                "导航",
                "选择",
                "文本输入",
                "场景",
            ],
            "返回顶部",
        ),
    ] {
        window.window().set_size(LogicalSize::new(width, 800.0));
        slint::select_bundled_translation(locale).unwrap();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let directory = ElementHandle::find_by_element_type_name(&window, "GitHubLink")
            .find(|element| element.accessible_label().as_deref() == Some(labels[0]))
            .unwrap();
        let directory_position = directory.absolute_position();
        for label in labels {
            let link = ElementHandle::find_by_element_type_name(&window, "GitHubLink")
                .find(|element| element.accessible_label().as_deref() == Some(label))
                .unwrap();
            assert!(link.absolute_position().x + link.size().width <= width);
            link.mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
            let group = find("GitHubShowcaseGroup::heading");
            assert_eq!(group.accessible_label().as_deref(), Some(label));
            assert!(
                group.absolute_position().y >= 0.0 && group.absolute_position().y < 150.0,
                "{label}"
            );
            let sample = match label {
                "Actions" => Some("Try action"),
                "操作" => Some("执行操作"),
                "Navigation" => Some("Standalone link"),
                "导航" => Some("独立链接"),
                "Selection" => Some("Select all"),
                "选择" => Some("全选"),
                "Scenarios" => Some("General"),
                "场景" => Some("普通消息"),
                _ => None,
            };
            if let Some(sample) = sample {
                assert!(
                    ElementHandle::find_by_accessible_label(&window, sample)
                        .next()
                        .is_some(),
                    "{locale}: {sample}"
                );
            }
            ElementHandle::find_by_accessible_label(&window, back)
                .next()
                .unwrap()
                .mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
            assert_eq!(directory.absolute_position(), directory_position);
        }
    }
    window.window().set_size(LogicalSize::new(1000.0, 800.0));
    slint::select_bundled_translation("en").unwrap();
    ElementHandle::find_by_element_type_name(&window, "GitHubLink")
        .find(|element| element.accessible_label().as_deref() == Some("Selection"))
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let page_scroll = playground::document(&window);
    let short = playground::reveal(
        &window,
        &page_scroll,
        "GitHubPrMenuShowcase::short-rules-button",
    );
    page_scroll.scroll(0.0, 220.0 - short.absolute_position().y);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let flow = find("GitHubPrMenuShowcase::flow");
    assert!(short.absolute_position().y + short.size().height <= flow.absolute_position().y);
    short.mock_single_click(PointerEventButton::Left);
    find("GitHubPullRequestFlow::rule-btn").mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(
        find("GitHubPullRequestMenuOverlay::filter-menu")
            .size()
            .height,
        112.0
    );
    find("GitHubPullRequestFlow::rule-btn").mock_single_click(PointerEventButton::Left);
    find("GitHubPrMenuShowcase::long-rules-button").mock_single_click(PointerEventButton::Left);
    let sort = find("GitHubPullRequestFlow::sort-btn");
    sort.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let popup = find("GitHubPullRequestMenuOverlay::filter-menu");
    assert!(
        (popup.absolute_position().y - sort.absolute_position().y - sort.size().height - 4.0).abs()
            < 0.5
    );
    sort.mock_single_click(PointerEventButton::Left);
    page_scroll.scroll(0.0, 100000.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    ElementHandle::find_by_accessible_label(&window, "Back to top")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    ElementHandle::find_by_element_type_name(&window, "GitHubLink")
        .find(|element| element.accessible_label().as_deref() == Some("Actions"))
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let scroll = playground::document(&window);
    let start = playground::reveal(
        &window,
        &scroll,
        "GitHubUiComponentsGallery::start-action-button",
    );
    scroll.scroll(0.0, 280.0 - start.absolute_position().y);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));

    let section = find("GitHubUiComponentsGallery::action-menu-section");
    let section_position = section.absolute_position();
    let section_size = section.size();
    let start = find("GitHubUiComponentsGallery::start-action-button");
    start.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(start.accessible_checked(), Some(true));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!((menu.absolute_position().x - start.absolute_position().x).abs() < 0.5);
    assert!(
        (menu.absolute_position().y - start.absolute_position().y - start.size().height - 4.0)
            .abs()
            < 0.5
    );
    assert_eq!(section.absolute_position(), section_position);
    assert_eq!(section.size(), section_size);

    find("GitHubUiComponentsGallery::menu-dismiss-field")
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsGallery::action-menu")
            .next()
            .is_none(),
        "focusing a shared input must dismiss the root-mounted ActionMenu"
    );

    start.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    start.mock_single_click(PointerEventButton::Left);
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsGallery::action-menu")
            .next()
            .is_none(),
        "a second click on the active trigger must close the menu"
    );

    let end = find("GitHubUiComponentsGallery::end-action-button");
    end.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!(
        (menu.absolute_position().x + menu.size().width
            - end.absolute_position().x
            - end.size().width)
            .abs()
            < 0.5
    );
    assert!(
        (menu.absolute_position().y - end.absolute_position().y - end.size().height - 4.0).abs()
            < 0.5
    );

    window.window().set_size(LogicalSize::new(760.0, 700.0));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let end = find("GitHubUiComponentsGallery::end-action-button");
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!(
        (menu.absolute_position().x + menu.size().width
            - end.absolute_position().x
            - end.size().width)
            .abs()
            < 0.5,
        "menu must remain end-aligned after a window resize"
    );
    assert!(
        (menu.absolute_position().y - end.absolute_position().y - end.size().height - 4.0).abs()
            < 0.5,
        "menu must remain below its trigger after a window resize"
    );

    let resized_section_position = section.absolute_position();
    let resized_section_size = section.size();
    let above = find("GitHubUiComponentsGallery::above-action-button");
    above.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!((menu.absolute_position().x - above.absolute_position().x).abs() < 0.5);
    assert!(
        (menu.absolute_position().y + menu.size().height - above.absolute_position().y + 4.0).abs()
            < 0.5
    );
    assert_eq!(section.absolute_position(), resized_section_position);
    assert_eq!(section.size(), resized_section_size);
}
