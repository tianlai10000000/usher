use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::{ComponentHandle, Global, LogicalSize};

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    #[library_path(material) = "material-1.18.1/material.slint"]
    import { GitHubDialog } from "ui/components/dialog/github_dialog.slint";
    import { GitHubDialogEnhance } from "ui/components/dialog/dialog_enhance.slint";
    import { DialogManager } from "ui/components/dialog/dialog_manager.slint";
    import { I18n } from "@i18n";
    export { DialogManager }

    export component DialogGeometryHarness inherits Window {
        width: 800px; height: 600px;
        in property <length> test-padding: 16px;
        in property <bool> footer: true;
        in property <bool> subtitle: false;
        out property <length> body-x: shell.body-x;
        out property <length> body-y: shell.body-y;
        out property <length> body-width: shell.body-width;
        out property <length> body-height: shell.body-height;
        out property <length> content-x: shell.content-x;
        out property <length> content-y: shell.content-y;
        out property <length> content-width: shell.content-width;
        out property <length> content-height: shell.content-height;
        shell := GitHubDialog {
            dialog-visible: true;
            dialog-width: 420px; dialog-height: 320px;
            title: I18n.settings-ai-agent-edit-title;
            subtitle: root.subtitle ? I18n.settings-ai-agent-api-key-caption : "";
            body-padding: root.test-padding; show-footer: root.footer;
            primary-label: I18n.common-save;
        }
    }

    export component DialogEnhanceHarness inherits Window {
        width: 800px; height: 600px;
        out property <string> action-copy: I18n.common-save;
        out property <string> title-copy: I18n.settings-ai-agent-edit-title;
        adapter := GitHubDialogEnhance { }
    }
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

fn find(window: &impl ComponentHandle, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing production element {id}"))
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.6,
        "actual {actual}, expected {expected}"
    );
}

#[test]
fn shared_dialog_raw_body_and_padded_content_preserve_both_chrome_variants() {
    i_slint_backend_testing::init_no_event_loop();
    let window = DialogGeometryHarness::new().unwrap();
    window.window().set_size(LogicalSize::new(800.0, 600.0));
    window.show().unwrap();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        for padding in [0.0, 16.0] {
            window.set_test_padding(padding);
            for subtitle in [false, true] {
                window.set_subtitle(subtitle);
                for footer in [false, true] {
                    window.set_footer(footer);
                    settle();
                    let surface = find(&window, "GitHubDialog::dialog-surface");
                    let position = surface.absolute_position();
                    let size = surface.size();
                    let header_height = if subtitle { 72.0 } else { 56.0 };
                    let footer_height = if footer { 64.0 } else { 0.0 };
                    near(window.get_body_x(), position.x);
                    near(window.get_body_y(), position.y + header_height);
                    near(window.get_body_width(), size.width);
                    near(
                        window.get_body_height(),
                        size.height - header_height - footer_height,
                    );
                    near(window.get_content_x(), window.get_body_x() + padding);
                    near(window.get_content_y(), window.get_body_y() + padding);
                    near(
                        window.get_content_width(),
                        window.get_body_width() - padding * 2.0,
                    );
                    near(
                        window.get_content_height(),
                        window.get_body_height() - padding * 2.0,
                    );
                }
            }
        }
    }
}

fn configure(window: &DialogEnhanceHarness, content: &str) {
    let manager = DialogManager::get(window);
    manager.set_visible(false);
    settle();
    manager.set_width_px(420.0);
    manager.set_height_px(320.0);
    manager.set_width_is_percent(false);
    manager.set_height_is_percent(false);
    manager.set_title(window.get_title_copy());
    manager.set_default_action(window.get_action_copy());
    manager.set_content(content.into());
    manager.set_visible(true);
    settle();
}

fn scroll(window: &DialogEnhanceHarness, delta: f32) {
    find(window, "GitHubScrollView::scroll-area").scroll(0.0, delta);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(600));
    settle();
}

#[test]
fn generic_dialog_long_body_scrolls_to_chrome_edges_with_moving_padding_in_both_locales() {
    i_slint_backend_testing::init_no_event_loop();
    let window = DialogEnhanceHarness::new().unwrap();
    window.window().set_size(LogicalSize::new(800.0, 600.0));
    window.show().unwrap();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        let content = (0..48)
            .map(|index| format!("{index:02} · Latin 与中文 😀 /long/path/with-no-breaks\n"))
            .collect::<String>();
        configure(&window, &content);
        let surface = find(&window, "GitHubDialog::dialog-surface");
        let viewport = find(&window, "GitHubScrollView::scroll-area");
        let layout = find(&window, "GitHubDialogEnhance::body-content");
        let text = find(&window, "GitHubDialogEnhance::content-text");
        let top = surface.absolute_position().y + 56.0;
        let bottom = surface.absolute_position().y + surface.size().height - 64.0;
        near(viewport.absolute_position().y, top);
        near(viewport.size().height, bottom - top);
        near(text.absolute_position().y, top + 16.0);
        assert!(text.size().height > viewport.size().height * 2.0);
        near(layout.size().height, text.size().height + 32.0);

        scroll(&window, -96.0);
        assert!(text.absolute_position().y < top - 1.0);
        assert!(text.absolute_position().y + text.size().height > bottom);
        // A middle slice can use the complete body, without permanent top or
        // bottom safe-area bands cutting through the native text document.
        near(viewport.absolute_position().y, top);
        near(
            viewport.absolute_position().y + viewport.size().height,
            bottom,
        );
        near(
            text.absolute_position().y - layout.absolute_position().y,
            16.0,
        );

        scroll(&window, -100_000.0);
        near(
            text.absolute_position().y + text.size().height,
            bottom - 16.0,
        );
        near(layout.absolute_position().y + layout.size().height, bottom);
        scroll(&window, 100_000.0);
        near(text.absolute_position().y, top + 16.0);
        near(layout.absolute_position().y, top);
    }
}

#[test]
fn generic_dialog_short_body_retains_safe_padding_without_artificial_scroll() {
    i_slint_backend_testing::init_no_event_loop();
    let window = DialogEnhanceHarness::new().unwrap();
    window.window().set_size(LogicalSize::new(800.0, 600.0));
    window.show().unwrap();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        configure(&window, "Short body / 简短内容 😀");
        let surface = find(&window, "GitHubDialog::dialog-surface");
        let viewport = find(&window, "GitHubScrollView::scroll-area");
        let text = find(&window, "GitHubDialogEnhance::content-text");
        let body_top = surface.absolute_position().y + 56.0;
        let body_bottom = surface.absolute_position().y + surface.size().height - 64.0;
        near(viewport.absolute_position().y, body_top);
        near(viewport.size().height, body_bottom - body_top);
        near(text.absolute_position().y, body_top + 16.0);
        assert!(text.absolute_position().y + text.size().height <= body_bottom - 16.0);
        let before = text.absolute_position();
        scroll(&window, -10_000.0);
        near(text.absolute_position().y, before.y);
        scroll(&window, 10_000.0);
        near(text.absolute_position().y, before.y);
    }
}
