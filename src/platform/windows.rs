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
    fn MessageBoxW(window: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

pub(crate) fn show_startup_error(message: &str) {
    let text = format!("Usher could not start.\n\n{message}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let caption = "Usher Startup Error\0".encode_utf16().collect::<Vec<_>>();
    // MB_OK | MB_ICONERROR | MB_SETFOREGROUND
    unsafe {
        MessageBoxW(
            0,
            text.as_ptr(),
            caption.as_ptr(),
            0x0000_0010 | 0x0001_0000,
        );
    }
}

/// Queue the user-configured Windows default system sound.
pub(crate) fn play_ui_click() {
    // MB_OK is asynchronous and honors the sound selected/disabled by the user.
    let played = unsafe { MessageBeep(0) };
    if played == 0 {
        log::debug!("[Sound] Windows MessageBeep(MB_OK) failed");
    }
}
