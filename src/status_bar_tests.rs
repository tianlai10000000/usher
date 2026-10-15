use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Global};

use crate::{AppState, MainWindow, UpdateLogic};

mod fixture {
    slint::slint! {
        #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
        #[library_path(material) = "material-1.18.1/material.slint"]
        import { GitHubStatusBar } from "ui/components/status_bar.slint";
        import { UpdateLogic } from "ui/components/update/update.slint";
        export { UpdateLogic }
        export component GitHubStatusBarHarness inherits Window {
            width: 1000px;
            height: 600px;
            out property <string> popup-title: bar.popup-title;
            out property <string> popup-content: bar.popup-content;
            bar := GitHubStatusBar {
                x: 0px;
                y: parent.height - self.height;
                width: parent.width;
            }
        }
    }
}

fn popup_text(window: &MainWindow) -> (ElementHandle, ElementHandle) {
    let title = ElementHandle::find_by_element_id(window, "GitHubStatusBar::popup-title-text")
        .next()
        .unwrap();
    let content = ElementHandle::find_by_element_id(window, "GitHubStatusBar::popup-content-text")
        .next()
        .unwrap();
    (title, content)
}

fn click(window: &MainWindow, label: &str) {
    ElementHandle::find_by_accessible_label(window, label)
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
}

#[test]
fn mounted_network_and_version_popups_follow_locale_and_current_feature_state() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1000.0, 600.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    AppState::get(&window).set_network_online(true);
    AppState::get(&window).set_version("fixture-1".into());
    crate::i18n::apply_preference("en").unwrap();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    click(&window, "Network connected");
    let (title, content) = popup_text(&window);
    assert_eq!(
        title.accessible_label().unwrap(),
        crate::i18n::tr("status.network_status").as_ref()
    );
    assert_eq!(content.accessible_label().unwrap(), "Network connected");
    crate::i18n::apply_preference("zh-CN").unwrap();
    assert!(
        title.is_valid() && content.is_valid(),
        "locale switch must keep the same popup nodes"
    );
    assert_eq!(
        title.accessible_label().unwrap(),
        crate::i18n::tr("status.network_status").as_ref()
    );
    AppState::get(&window).set_network_online(false);
    assert_eq!(
        content.accessible_label().unwrap(),
        crate::i18n::tr("status.network_disconnected").as_ref()
    );
    click(
        &window,
        crate::i18n::tr("status.network_disconnected").as_ref(),
    );
    click(
        &window,
        &format!("{}: fixture-1", crate::i18n::tr("common.version")),
    );
    let (title, content) = popup_text(&window);
    AppState::get(&window).set_version("fixture-2".into());
    assert!(content.accessible_label().unwrap().contains("fixture-2"));
    crate::i18n::apply_preference("en").unwrap();
    assert!(title.is_valid() && content.is_valid());
    assert_eq!(title.accessible_label().unwrap(), "Version");
    assert_eq!(content.accessible_label().unwrap(), "Version: fixture-2");
}

#[test]
fn mounted_download_and_update_check_popups_keep_live_catalog_and_semantic_progress() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let host = MainWindow::new().unwrap();
    let harness = fixture::GitHubStatusBarHarness::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    let logic = harness.global::<fixture::UpdateLogic>();
    logic.set_checking_update(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_element_id(&harness, "GitHubStatusBar::update-check-item")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    assert_eq!(
        harness.get_popup_content(),
        crate::i18n::tr("status.checking_update").as_ref()
    );
    crate::i18n::apply_preference("zh-CN").unwrap();
    assert_eq!(
        harness.get_popup_content(),
        crate::i18n::tr("status.checking_update").as_ref()
    );
    logic.set_checking_update(false);
    // Slint changed handlers run on the event-loop turn, not in the Rust setter.
    // A zero-duration testing tick runs those handlers and updates conditionals.
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(harness.get_popup_title(), "");
    assert_eq!(harness.get_popup_content(), "");
    logic.set_is_downloading(true);
    logic.set_new_version("9.8.7".into());
    logic.set_download_progress(0.25);
    crate::update::download_status::present(
        &host,
        crate::update::updater::DownloadStatus::Bytes {
            downloaded: 1_048_576,
            total: 4_194_304,
        },
    );
    logic.set_download_status(host.global::<UpdateLogic>().get_download_status());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_element_id(&harness, "GitHubStatusBar::download-progress-item")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    assert!(harness.get_popup_content().contains("25%"));
    assert!(harness.get_popup_content().contains("9.8.7"));
    let same_content =
        ElementHandle::find_by_element_id(&harness, "GitHubStatusBar::popup-content-text")
            .next()
            .unwrap();
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        crate::update::download_status::present(
            &host,
            crate::update::updater::DownloadStatus::Bytes {
                downloaded: 3_145_728,
                total: 4_194_304,
            },
        );
        crate::update::updater::sync_language(&host);
        logic.set_download_status(host.global::<UpdateLogic>().get_download_status());
        logic.set_download_progress(0.75);
        assert!(same_content.is_valid());
        assert_eq!(
            harness.get_popup_title(),
            crate::i18n::tr("status.download_progress").as_ref()
        );
        assert!(harness.get_popup_content().contains("75%"));
        assert!(
            harness
                .get_popup_content()
                .contains(host.global::<UpdateLogic>().get_download_status().as_str())
        );
    }
    logic.set_is_downloading(false);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        !same_content.is_valid(),
        "completed download must unmount the open popup on the next UI turn"
    );
    assert_eq!(harness.get_popup_title(), "");
    assert_eq!(harness.get_popup_content(), "");
    crate::i18n::apply_preference("en").unwrap();
}
