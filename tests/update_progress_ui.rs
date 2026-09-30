use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, UpdateLogic};

#[test]
fn mock_update_progress_grows_from_left_in_dialog_and_status_bar() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_platform_is_macos(false);
    let update = window.global::<UpdateLogic>();
    update.set_new_version("0.0.9-alpha.3".into());
    update.set_is_downloading(true);

    for (locale, cancel_label) in [("en", "Cancel download"), ("zh-CN", "取消下载")] {
        slint::select_bundled_translation(locale).unwrap();
        for (width, height) in [(800.0, 600.0), (480.0, 600.0)] {
            window.window().dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(width, height),
            });
            for progress in [0.0, 0.15, 0.5, 1.0] {
                update.set_download_progress(progress);
                update.set_total_bytes(20_000_000);
                update.set_downloaded_bytes((20_000_000.0 * progress) as i32);
                // Resolve the animation target before advancing the mock clock.
                i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
                let bars: Vec<_> =
                    ElementHandle::find_by_element_type_name(&window, "GitHubProgressBar")
                        .collect();
                for bar in &bars {
                    let _ = bar.size();
                    for fill in bar
                        .query_descendants()
                        .match_id("GitHubProgressBar::fill")
                        .find_all()
                    {
                        let _ = fill.size();
                    }
                }
                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
                let mut checked = 0;
                for bar in bars {
                    for fill in bar
                        .query_descendants()
                        .match_id("GitHubProgressBar::fill")
                        .find_all()
                    {
                        assert!(
                            (fill.absolute_position().x - bar.absolute_position().x).abs() < 0.5,
                            "fill must stay left-aligned: locale={locale}, width={width}, progress={progress}"
                        );
                        assert!(
                            (fill.size().width - bar.size().width * progress).abs() < 0.5,
                            "fill width must match progress: locale={locale}, width={width}, progress={progress}"
                        );
                        checked += 1;
                    }
                }
                assert_eq!(
                    checked,
                    if progress == 0.0 { 1 } else { 2 },
                    "status bar is determinate; dialog is indeterminate only while preparing"
                );
                assert!(
                    ElementHandle::find_by_accessible_label(&window, cancel_label)
                        .next()
                        .is_some(),
                    "mounted dialog must refresh its locale"
                );
            }
        }
    }
}
