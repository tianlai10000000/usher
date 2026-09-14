//! Shape visible source rows separately from their explicit visual line pitch.
use parley::style::{FontFamily, FontFamilyName, OverflowWrap};
use parley::{FontContext, LayoutContext, StyleProperty};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;

struct LogLayout {
    fonts: FontContext,
    context: LayoutContext<()>,
}

impl LogLayout {
    fn new() -> Self {
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

    fn wrap(&mut self, text: &str, width: f32) -> Vec<SharedString> {
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
    fn root_ui_spaces_continuations_without_repeating_the_source_number() {
        use i_slint_backend_testing::ElementHandle;
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        ui.global::<crate::JobLogLayout>().on_wrap_lines(|text, _| {
            if text != "fixture" {
                return ModelRc::default();
            }
            ModelRc::new(VecModel::from(vec![
                SharedString::from("first visual line"),
                SharedString::from("second visual line"),
                SharedString::from("third visual line"),
            ]))
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
    let layout = RefCell::new(LogLayout::new());
    ui.global::<crate::JobLogLayout>()
        .on_wrap_lines(move |text, width| {
            ModelRc::new(VecModel::from(
                layout.borrow_mut().wrap(text.as_str(), width),
            ))
        });
}
