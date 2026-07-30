//! Windows theme and system-sound integration.

/// Read Windows' per-user application theme preference.
pub(crate) fn detect_system_dark_mode() -> bool {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => true,
        Ok(dark_light::Mode::Light | dark_light::Mode::Unspecified) => false,
        Err(error) => {
            log::warn!("[Theme] Failed to detect the Windows system theme: {error}");
            false
        }
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(sound_type: u32) -> i32;
}

/// Queue the user-configured Windows default system sound.
pub(crate) fn play_ui_click() {
    // MB_OK is asynchronous and honors the sound selected/disabled by the user.
    let played = unsafe { MessageBeep(0) };
    if played == 0 {
        log::debug!("[Sound] Windows MessageBeep(MB_OK) failed");
    }
}
