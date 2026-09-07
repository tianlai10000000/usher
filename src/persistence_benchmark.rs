//! Opt-in P5 persistence/UI fixture; all files and messages are isolated and local.
use crate::{AppState, MainWindow};
use slint::{ComponentHandle, Global};
use std::sync::mpsc;
use std::time::Duration;
use usher_messages::{MessageCenter, MessageDraft};
use usher_state_store::{PersistentStore, Reduction};

struct Payload {
    revision: u64,
    bytes: Vec<u8>,
}
pub(crate) fn setup(ui: &MainWindow, center: &MessageCenter) {
    if std::env::var("USHER_P5_BENCHMARK").as_deref() != Ok("59")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let window = ui.as_weak();
    let center = center.clone();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let current = window.clone();
            let expected_unread = center.snapshot().unread_count + 200;
            let result = tokio::task::spawn_blocking(move || {
                let directory = crate::app_paths::config_dir().unwrap();
                let path = directory.join("p5-latest.bin");
                let output = path.clone();
                let (started, ready) = mpsc::channel();
                let store = PersistentStore::new(
                    Payload { revision: 0, bytes: Vec::new() },
                    |_: &Payload, revision: u64| {
                        let mut bytes = vec![(revision % 255) as u8; 64 * 1024];
                        bytes[..8].copy_from_slice(&revision.to_le_bytes());
                        Reduction::changed(Payload { revision, bytes }, ())
                    },
                    "p5-latest-fixture",
                    move |state| {
                        if state.revision % 501 == 1 {
                            let _ = started.send(state.revision);
                            std::thread::sleep(Duration::from_secs(1));
                        }
                        std::fs::write(&output, &state.bytes).map_err(|error| error.to_string())
                    },
                    |_, error| log::error!("[P5][59] persistence_error={error}"),
                ).unwrap();
                let mut valid = true;
                for round in 0..4u64 {
                    let weak = current.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            let settings = AppState::get(&ui);
                            for index in 0..100 { settings.invoke_toggle_ui_sound_effects(index % 2 == 0); }
                            settings.invoke_toggle_ui_sound_effects(round % 2 == 0);
                        }
                    });
                    for index in 0..50 {
                        valid &= center.publish(MessageDraft::new(format!("P5 {round}-{index}"), "Fixture message".repeat(32))
                            .source("P5").desktop(false)).is_ok();
                    }
                    let first = round * 501 + 1;
                    store.dispatch(first);
                    valid &= ready.recv_timeout(Duration::from_secs(3)).ok() == Some(first);
                    for revision in first + 1..=first + 500 { store.dispatch(revision); }
                    valid &= store.flush().is_ok();
                    let bytes = std::fs::read(&path).unwrap_or_default();
                    let final_revision = bytes.get(..8).and_then(|bytes| bytes.try_into().ok()).map(u64::from_le_bytes);
                    valid &= final_revision == Some(first + 500);
                    log::info!("[P5][59] round={round} revision={} persisted={final_revision:?}", store.snapshot().revision());
                }
                store.shutdown();
                valid
            }).await.unwrap_or(false);
            tokio::time::sleep(Duration::from_secs(2)).await;
            let settings_ok = tokio::task::spawn_blocking(|| {
                let path = crate::app_paths::config_dir().unwrap().join("settings.toml");
                std::fs::read_to_string(path).ok().and_then(|text| toml::from_str::<toml::Value>(&text).ok())
                    .and_then(|value| value.get("ui_sound_effects").and_then(toml::Value::as_bool)) == Some(false)
            }).await.unwrap_or(false);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = window.upgrade() {
                    let ui_ok = ui.get_message_unread_count() as usize == expected_unread;
                    log::info!("[P5][59] result persistence_ok={result} settings_ok={settings_ok} ui_ok={ui_ok} unread={}", ui.get_message_unread_count());
                    if result && settings_ok && ui_ok { log::info!("[P5][59] complete"); }
                }
            });
        });
    });
}
