//! Linux desktop theme and sound integration.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static CLICK_PLAYING: AtomicBool = AtomicBool::new(false);

/// Read the desktop theme through the XDG Desktop Portal.
pub(crate) fn detect_system_dark_mode() -> bool {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => true,
        Ok(dark_light::Mode::Light | dark_light::Mode::Unspecified) => false,
        Err(error) => {
            log::warn!("[Theme] Failed to detect the Linux desktop theme: {error}");
            false
        }
    }
}

/// Play a desktop click without blocking the Slint event loop.
pub(crate) fn play_ui_click() {
    if CLICK_PLAYING.swap(true, Ordering::AcqRel) {
        return;
    }

    if let Err(error) = std::thread::Builder::new()
        .name("ui-click-sound".to_string())
        .spawn(|| {
            if !play_sound_theme_event() && !play_fallback_tone() {
                log::debug!(
                    "[Sound] No Linux sound-theme or command-line audio player is available"
                );
            }
            CLICK_PLAYING.store(false, Ordering::Release);
        })
    {
        CLICK_PLAYING.store(false, Ordering::Release);
        log::debug!("[Sound] Failed to start Linux UI-click worker: {error}");
    }
}

fn play_sound_theme_event() -> bool {
    Command::new("canberra-gtk-play")
        .args([
            "--id=button-pressed",
            "--description=Usher page Back",
            "--cache-control=permanent",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn play_fallback_tone() -> bool {
    let wave = click_wave();
    for player in ["pw-play", "paplay", "aplay"] {
        let Ok(mut child) = Command::new(player)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };

        let wrote_wave = child
            .stdin
            .take()
            .is_some_and(|mut input| input.write_all(&wave).is_ok());
        if wrote_wave && child.wait().is_ok_and(|status| status.success()) {
            return true;
        }
    }
    false
}

fn click_wave() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22_050;
    const SAMPLE_COUNT: usize = 992; // 45 ms
    const FREQUENCY: f32 = 880.0;
    const AMPLITUDE: f32 = 0.16;

    let data_size = (SAMPLE_COUNT * size_of::<i16>()) as u32;
    let mut wave = Vec::with_capacity(44 + data_size as usize);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + data_size).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wave.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_size.to_le_bytes());

    for sample in 0..SAMPLE_COUNT {
        let progress = sample as f32 / SAMPLE_COUNT as f32;
        let envelope = (1.0 - progress).powi(3);
        let phase = 2.0 * std::f32::consts::PI * FREQUENCY * sample as f32 / SAMPLE_RATE as f32;
        let value = (phase.sin() * envelope * AMPLITUDE * i16::MAX as f32) as i16;
        wave.extend_from_slice(&value.to_le_bytes());
    }
    wave
}

#[cfg(test)]
mod tests {
    use super::click_wave;

    #[test]
    fn fallback_click_is_a_well_formed_pcm_wave() {
        let wave = click_wave();
        assert_eq!(&wave[0..4], b"RIFF");
        assert_eq!(&wave[8..12], b"WAVE");
        assert_eq!(&wave[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(wave[4..8].try_into().unwrap()) as usize + 8,
            wave.len()
        );
        assert_eq!(
            u32::from_le_bytes(wave[40..44].try_into().unwrap()) as usize + 44,
            wave.len()
        );
    }
}
