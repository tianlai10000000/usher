//! Shape visible source rows separately from their explicit visual line pitch.
use parley::style::{FontFamily, FontFamilyName, OverflowWrap};
use parley::{FontContext, LayoutContext, StyleProperty};
use slint::SharedString;
#[cfg(test)]
use slint::{ComponentHandle, ModelRc, VecModel};

pub(super) struct LogLayout {
    fonts: FontContext,
    context: LayoutContext<()>,
}

impl LogLayout {
    pub(super) fn collection(&self) -> parley::fontique::Collection {
        self.fonts.collection.clone()
    }
    pub(super) fn with_collection(collection: parley::fontique::Collection) -> Self {
        Self {
            fonts: FontContext {
                collection,
                source_cache: Default::default(),
            },
            context: LayoutContext::new(),
        }
    }
    pub(super) fn new() -> Self {
        Self {
            #[cfg(not(test))]
            fonts: FontContext {
                collection: slint::fontique_010::shared_collection(),
                source_cache: Default::default(),
            },
            #[cfg(test)]
            fonts: FontContext::new(),
            context: LayoutContext::new(),
        }
    }

    pub(super) fn wrap(&mut self, text: &str, width: f32) -> Vec<SharedString> {
        // Parley shapes empty text with a synthetic space whose range is 0..1.
        // Keep blank source rows without slicing that range into the original.
        if text.is_empty() {
            return vec![SharedString::default()];
        }
        let mut builder = self
            .context
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(FontFamily::Single(FontFamilyName::named("monospace")));
        builder.push_default(StyleProperty::FontSize(12.0));
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(width.max(1.0)));
        layout
            .lines()
            .map(|line| {
                text[line.text_range()]
                    .trim_end_matches(['\r', '\n'])
                    .into()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rows_remain_one_visual_line_across_resize_and_layout_reuse() {
        let mut layout = LogLayout::new();
        for width in [800.0, 160.0, 0.0, 800.0] {
            assert_eq!(layout.wrap("", width), vec![SharedString::default()]);
            assert_eq!(layout.wrap("next", width).concat(), "next");
            assert_eq!(layout.wrap("", width), vec![SharedString::default()]);
        }
    }

    #[test]
    fn wrapping_preserves_blank_lines_and_unicode_around_line_breaks() {
        let mut layout = LogLayout::new();
        for width in [800.0, 160.0, 1.0] {
            for text in ["\n", "\r\n", "\n\n", "\n消息\r\n\nnext\n"] {
                let lines = layout.wrap(text, width);
                assert!(!lines.is_empty());
                assert_eq!(lines.concat(), text.replace(['\r', '\n'], ""));
                assert_eq!(lines.last(), Some(&SharedString::default()));
            }
        }
    }

    #[test]
    fn wrapping_preserves_json_unicode_and_long_paths_on_resize() {
        let text = r#"[hook.finished] {"detail":"消息发送失败 /private/var/folders/very-long-path/hook.mts:136", "exit_code":1}"#;
        let mut layout = LogLayout::new();
        let wide = layout.wrap(text, 800.0);
        let narrow = layout.wrap(text, 160.0);
        assert!(narrow.len() > wide.len());
        assert_eq!(wide.concat(), text);
        assert_eq!(narrow.concat(), text);
        assert_eq!(layout.wrap(text, 800.0), wide);
    }

    #[test]
    fn root_ui_renders_blank_log_rows_with_the_real_layout_callback() {
        use i_slint_backend_testing::ElementHandle;
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        setup(&ui);
        ui.set_current_route(crate::Route::JobRun);
        ui.set_job_run_steps(ModelRc::new(VecModel::from(vec![crate::JobStepItem {
            name: "Blank log fixture".into(),
            status: "completed".into(),
            conclusion: "failure".into(),
            has_log: true,
            log_loaded: true,
            default_expanded: true,
            log_lines: ModelRc::new(VecModel::from(vec![
                Default::default(),
                crate::JobLogLineItem {
                    text: "Messages remain readable".into(),
                    ..Default::default()
                },
                Default::default(),
            ])),
            ..Default::default()
        }])));
        for locale in ["en", "zh-CN"] {
            crate::i18n::apply_preference(locale).unwrap();
            for width in [1200.0, 600.0] {
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::Resized {
                        size: slint::LogicalSize::new(width, 900.0),
                    });
                for label in ["1", "3", "Messages remain readable"] {
                    assert_eq!(
                        ElementHandle::find_by_accessible_label(&ui, label).count(),
                        1,
                        "log row {label:?} at {width} in {locale}"
                    );
                }
            }
        }
        crate::i18n::apply_preference("en").unwrap();
    }

    #[test]
    fn root_ui_spaces_continuations_without_repeating_the_source_number() {
        use i_slint_backend_testing::ElementHandle;
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        ui.global::<crate::JobLogLayout>()
            .on_layout_window(|_, _, _, _, _, _, _| crate::JobLogWindow {
                content_height: 80.0,
                rows: ModelRc::new(VecModel::from(vec![crate::JobLogVirtualRow {
                    line_number: 1,
                    extent: 80.0,
                    visual_lines: ModelRc::new(VecModel::from(vec![
                        SharedString::from("first visual line"),
                        SharedString::from("second visual line"),
                        SharedString::from("third visual line"),
                    ])),
                    ..Default::default()
                }])),
                ..Default::default()
            });
        ui.set_current_route(crate::Route::JobRun);
        ui.set_job_run_steps(ModelRc::new(VecModel::from(vec![crate::JobStepItem {
            name: "Layout fixture".into(),
            status: "completed".into(),
            conclusion: "failure".into(),
            has_log: true,
            log_loaded: true,
            default_expanded: true,
            log_lines: ModelRc::new(VecModel::from(vec![
                crate::JobLogLineItem {
                    text: "fixture".into(),
                    ..Default::default()
                },
                Default::default(),
                Default::default(),
                Default::default(),
            ])),
            ..Default::default()
        }])));
        for width in [1200.0, 600.0] {
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::Resized {
                    size: slint::LogicalSize::new(width, 900.0),
                });
            let first = ElementHandle::find_by_accessible_label(&ui, "first visual line")
                .next()
                .expect("first visual line is mounted");
            let second = ElementHandle::find_by_accessible_label(&ui, "second visual line")
                .next()
                .expect("second visual line is mounted");
            let third = ElementHandle::find_by_accessible_label(&ui, "third visual line")
                .next()
                .expect("third visual line is mounted");
            assert!(
                (second.absolute_position().y - first.absolute_position().y - 20.0).abs() < 0.1
            );
            assert!(
                (third.absolute_position().y - second.absolute_position().y - 20.0).abs() < 0.1
            );
            assert!(first.size().height <= 20.0);
            assert_eq!(ElementHandle::find_by_accessible_label(&ui, "1").count(), 1);
        }
    }
}

pub(super) fn setup(ui: &crate::MainWindow) {
    super::log_viewport::setup(ui);
}
