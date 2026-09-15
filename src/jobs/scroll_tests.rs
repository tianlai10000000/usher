use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

fn move_pointer(ui: &crate::MainWindow, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(x, y),
    });
}

fn wheel(ui: &crate::MainWindow, x: f32, y: f32, delta: f32) {
    move_pointer(ui, x, y);
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(x, y),
        delta_x: 0.0,
        delta_y: delta,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
}

#[test]
fn fixed_run_page_keeps_scrolling_inside_output() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        for width in [1200.0, 800.0, 1920.0] {
            for height in [600.0, 1200.0] {
                assert_nested_scroll_gestures(width, height, locale);
            }
        }
    }
    crate::i18n::apply_preference("en").unwrap();
}

fn assert_nested_scroll_gestures(width: f32, height: f32, locale: &str) {
    let ui = crate::MainWindow::new().unwrap();
    crate::i18n::apply_preference(locale).unwrap();
    super::setup_job_run(&ui);
    ui.set_current_route(crate::Route::JobRun);
    ui.set_job_run_steps(ModelRc::new(VecModel::from(vec![crate::JobStepItem {
        name: "Nested scroll fixture".into(),
        status: "completed".into(),
        conclusion: "failure".into(),
        has_log: true,
        log_loaded: true,
        default_expanded: true,
        log_lines: ModelRc::new(VecModel::from(
            (0..100)
                .map(|index| crate::JobLogLineItem {
                    text: format!("scroll fixture line {index}").into(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        )),
        ..Default::default()
    }])));
    ui.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width, height),
    });
    let header = ElementHandle::find_by_accessible_label(&ui, "Nested scroll fixture")
        .find(|element| element.accessible_checkable().is_some())
        .expect("step header");
    let bar = ElementHandle::find_by_element_id(&ui, "GitHubScrollView::scrollbar-hit")
        .find(|element| element.absolute_position().y > header.absolute_position().y)
        .expect("log scrollbar");
    let bar_position = bar.absolute_position();
    let run_header = ElementHandle::find_by_element_id(&ui, "GitHubJobRunSurface::run-header")
        .next()
        .expect("run header");
    assert!(
        run_header.absolute_position().y < 100.0,
        "run header must stay at the top"
    );
    assert!(
        run_header.absolute_position().y + run_header.size().height <= header.absolute_position().y,
        "run header must not overlap the output frame"
    );
    assert!(
        bar_position.y + bar.size().height <= height,
        "output must fit the window"
    );
    assert!(
        bar_position.y + bar.size().height > height - 100.0,
        "single-step logs must fill the output frame, including tall windows"
    );
    wheel(
        &ui,
        bar_position.x - 100.0,
        header.absolute_position().y - 20.0,
        -80.0,
    );
    let header_before = header.absolute_position();
    let bar_position = bar.absolute_position();
    let x = bar_position.x + bar.size().width / 2.0;
    let y = bar_position.y + 8.0;
    move_pointer(&ui, x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(x, y),
        button: PointerEventButton::Left,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));
    for offset in [20.0, 50.0, 80.0] {
        move_pointer(&ui, x, y + offset);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
        assert_eq!(
            header.absolute_position(),
            header_before,
            "inner drag must not move outer page"
        );
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(x, y + 80.0),
        button: PointerEventButton::Left,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    assert_eq!(header.absolute_position(), header_before);
    assert!(
        ElementHandle::find_by_accessible_label(&ui, "scroll fixture line 0")
            .next()
            .is_none(),
        "inner log must scroll"
    );

    // Output keeps bidirectional wheel scrolling without a page ancestor.
    wheel(&ui, x - 100.0, y + 50.0, 10000.0);
    assert_eq!(header.absolute_position(), header_before);
    assert!(
        ElementHandle::find_by_accessible_label(&ui, "scroll fixture line 0")
            .next()
            .is_some(),
        "inner wheel must return to the first line"
    );
    wheel(&ui, x - 100.0, y + 50.0, -240.0);
    assert_eq!(header.absolute_position(), header_before);
    assert!(
        ElementHandle::find_by_accessible_label(&ui, "scroll fixture line 0")
            .next()
            .is_none(),
        "inner wheel must scroll down again"
    );

    wheel(&ui, x - 100.0, header_before.y - 20.0, 10000.0);
    let page_top = header.absolute_position();
    wheel(&ui, x - 100.0, page_top.y - 20.0, -80.0);
    assert_eq!(
        header.absolute_position(),
        page_top,
        "the page must not scroll"
    );
    wheel(&ui, x - 100.0, y + 50.0, -100000.0);
    let last = ElementHandle::find_by_accessible_label(&ui, "scroll fixture line 99")
        .next()
        .unwrap_or_else(|| panic!("last log line must be reachable: {width}x{height}, {locale}"));
    assert!(last.absolute_position().y < bar_position.y + bar.size().height);
    assert_eq!(header.absolute_position(), page_top);
}

#[test]
fn fixed_output_frame_keeps_later_steps_reachable() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        for width in [800.0, 1440.0] {
            let ui = crate::MainWindow::new().unwrap();
            crate::i18n::apply_preference(locale).unwrap();
            super::setup_job_run(&ui);
            ui.set_current_route(crate::Route::JobRun);
            ui.set_job_run_steps(ModelRc::new(VecModel::from(
                (0..12)
                    .map(|index| crate::JobStepItem {
                        name: format!("Ordered step {index}").into(),
                        status: "completed".into(),
                        conclusion: "success".into(),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            )));
            ui.window().dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(width, 600.0),
            });
            let bar =
                ElementHandle::find_by_element_id(&ui, "GitHubJobRunJobFrame::steps-scrollbar")
                    .next()
                    .expect("step viewport scrollbar");
            let position = bar.absolute_position();
            wheel(&ui, position.x - 100.0, position.y + 40.0, -100000.0);
            let last = ElementHandle::find_by_accessible_label(&ui, "Ordered step 11")
                .next()
                .expect("last step");
            assert!(last.absolute_position().y >= position.y);
            assert!(
                last.absolute_position().y + last.size().height
                    <= position.y + bar.size().height + 1.0
            );
            wheel(&ui, position.x - 100.0, position.y + 40.0, 100000.0);
            let first = ElementHandle::find_by_accessible_label(&ui, "Ordered step 0")
                .next()
                .expect("first step");
            assert!(first.absolute_position().y >= position.y);
        }
    }
    crate::i18n::apply_preference("en").unwrap();
}

#[test]
fn loading_logs_state_hides_stale_rows_and_uses_a_full_viewport() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let ui = crate::MainWindow::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    ui.global::<crate::JobLogLayout>()
        .on_layout_window(|_, _, _, _, _, _, _| crate::JobLogWindow {
            content_height: 200.0,
            loading: true,
            rows: ModelRc::new(VecModel::from(vec![crate::JobLogVirtualRow {
                line: crate::JobLogLineItem {
                    text: "stale row must stay hidden".into(),
                    ..Default::default()
                },
                line_number: 1,
                visual_lines: ModelRc::new(VecModel::from(vec![
                    "stale row must stay hidden".into()
                ])),
                offset: 0.0,
                extent: 32.0,
            }])),
        });
    ui.set_current_route(crate::Route::JobRun);
    ui.set_job_run_steps(ModelRc::new(VecModel::from(vec![crate::JobStepItem {
        name: "Loading fixture".into(),
        status: "completed".into(),
        conclusion: "failure".into(),
        has_log: true,
        log_loaded: true,
        default_expanded: true,
        log_lines: ModelRc::new(VecModel::from(vec![crate::JobLogLineItem {
            text: "stale row must stay hidden".into(),
            ..Default::default()
        }])),
        ..Default::default()
    }])));
    ui.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1000.0, 800.0),
    });

    let loading = ElementHandle::find_by_accessible_label(&ui, "Loading logs…")
        .next()
        .expect("loading state label");
    assert!(loading.size().height >= 60.0);
    assert_eq!(
        ElementHandle::find_by_accessible_label(&ui, "stale row must stay hidden").count(),
        0,
        "stale rows must not overlap the loading state"
    );
    crate::i18n::apply_preference("en").unwrap();
}

#[test]
fn mixed_wrapped_rows_keep_thumb_height_stable_during_wheel_scrolling() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        for width in [800.0, 1200.0] {
            let ui = crate::MainWindow::new().unwrap();
            crate::i18n::apply_preference(locale).unwrap();
            super::setup_job_run(&ui);
            ui.set_current_route(crate::Route::JobRun);
            let lines = (0..120)
                .map(|index| crate::JobLogLineItem {
                    text: if index % 7 == 0 {
                        format!(
                            "long row {index}: {}",
                            "消息 /private/very-long-path/ {\"value\":42} ".repeat(30)
                        )
                        .into()
                    } else {
                        format!("short row {index}").into()
                    },
                    ..Default::default()
                })
                .collect::<Vec<_>>();
            ui.set_job_run_steps(ModelRc::new(VecModel::from(vec![crate::JobStepItem {
                name: "Stable thumb fixture".into(),
                status: "completed".into(),
                conclusion: "failure".into(),
                has_log: true,
                log_loaded: true,
                default_expanded: true,
                log_lines: ModelRc::new(VecModel::from(lines)),
                ..Default::default()
            }])));
            ui.window().dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(width, 900.0),
            });
            let header = ElementHandle::find_by_accessible_label(&ui, "Stable thumb fixture")
                .find(|element| element.accessible_checkable().is_some())
                .unwrap();
            let thumb = ElementHandle::find_by_element_id(&ui, "GitHubScrollView::scrollbar-thumb")
                .find(|element| element.absolute_position().y > header.absolute_position().y)
                .unwrap();
            let thumb_height = thumb.size().height;
            let initial = thumb.absolute_position();
            for direction in [-1.0, 1.0] {
                for _ in 0..30 {
                    move_pointer(&ui, initial.x - 100.0, initial.y + 30.0);
                    ui.window().dispatch_event(WindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(initial.x - 100.0, initial.y + 30.0),
                        delta_x: 0.0,
                        delta_y: direction * 120.0,
                    });
                    for _ in 0..12 {
                        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
                        assert!(
                            (thumb.size().height - thumb_height).abs() < 0.01,
                            "thumb height changed while scrolling: {locale}, {width}"
                        );
                    }
                }
            }
            assert!((thumb.absolute_position().y - initial.y).abs() < 1.0);
        }
    }
    crate::i18n::apply_preference("en").unwrap();
}
