//! Opt-in native acceptance entry; always uses the application's real bridge.
use slint::ComponentHandle;

pub(super) fn arm(window: &crate::MainWindow) -> bool {
    if std::env::var("USHER_UI_COMPONENTS_NATIVE_FIXTURE").as_deref() != Ok("1") {
        return false;
    }
    assert!(crate::app_paths::benchmark_offline_mode());
    let state_root = std::env::var_os("USHER_BENCHMARK_STATE_ROOT").expect("isolated state root");
    assert!(std::path::Path::new(&state_root).is_absolute());
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(250), move || {
        if let Some(window) = weak.upgrade() {
            window
                .window()
                .set_size(slint::LogicalSize::new(1100.0, 700.0));
            let locale =
                std::env::var("USHER_UI_COMPONENTS_FIXTURE_LOCALE").unwrap_or_else(|_| "en".into());
            window
                .global::<crate::AppState>()
                .invoke_set_language(locale.into());
            window.invoke_navigate(crate::Route::UiComponents);
        }
    });
    true
}

pub(super) fn trace(page: &usher_ui_components::GitHubUiComponentsWindow) {
    let weak = page.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        if let Some(page) = weak.upgrade() {
            log::info!(
                "[Playground][Fixture] active={} transient_open={} tooltip={:?}",
                page.get_route_active(),
                page.get_transient_open(),
                page.global::<usher_ui_components::FactoryTooltipState>()
                    .get_kind()
            );
        }
    });
}

pub(super) fn trace_pointer(host: &crate::MainWindow, x: f32, y: f32) {
    let window = i_slint_core::window::WindowInner::from_pub(host.window());
    let focused = window.focus_item.borrow().upgrade();
    let text_input = focused
        .as_ref()
        .is_some_and(|item| item.downcast::<i_slint_core::items::TextInput>().is_some());
    let selection = focused.as_ref().and_then(|item| {
        item.downcast::<i_slint_core::items::TextInput>()
            .map(|input| {
                let input = input.as_pin_ref();
                (
                    input.anchor_position_byte_offset(),
                    input.cursor_position_byte_offset(),
                )
            })
    });
    let contains = crate::transient_input::focused_text_input_contains_pointer(
        host.window(),
        slint::LogicalPosition::new(x, y),
    );
    log::info!(
        "[Playground][Fixture][Pointer] route={:?} logical=({x},{y}) focused={} text_input={text_input} contains={contains} selection={selection:?}",
        host.get_current_route(),
        focused.is_some()
    );
}
