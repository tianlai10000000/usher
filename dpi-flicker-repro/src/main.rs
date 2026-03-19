slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Optional: a tiny animation to differentiate "flash" from "layout reflow".
    // Toggle with: DPI_REPRO_ANIMATE=1
    if std::env::var_os("DPI_REPRO_ANIMATE").is_some() {
        use std::time::Duration;
        let ui_weak = ui.as_weak();
        let mut t: f32 = 0.0;
        slint::Timer::default().start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                t += 0.03;
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_phase(((t.sin() + 1.0) * 0.5) as f32);
                }
            },
        );
    }

    ui.run()
}
