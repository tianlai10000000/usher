use std::time::Duration;

use slint::{ComponentHandle, Global, Weak};

use crate::app_runtime::shared_runtime;
use crate::http::{self, RequestOptions};
use crate::{AppState, MainWindow};

/// Check network connectivity using HTTP request (more reliable than raw TCP)
fn check_online() -> bool {
    let runtime = shared_runtime();

    runtime.block_on(async {
        // Try multiple connectivity check endpoints
        let urls = [
            "http://connectivitycheck.gstatic.com/generate_204",
            "http://captive.apple.com/hotspot-detect.html",
            "http://www.msftconnecttest.com/connecttest.txt",
        ];

        for url in urls {
            if let Ok(resp) = http::send(
                http::get(url),
                RequestOptions::new("Check network connectivity")
                    .timeout(Duration::from_secs(3))
                    .allow_http_errors(),
            )
            .await
            {
                let status = resp.status().as_u16();
                if status == 204 || status == 200 {
                    return true;
                }
            }
        }
        false
    })
}

pub fn setup_network_monitor(app: &MainWindow) {
    // Optimistic initial value: the serial probe takes up to 9s offline, and it
    // used to run on the main thread before `ui.show()`. The monitor thread's
    // first iteration corrects the state within one probe round instead.
    AppState::get(app).set_network_online(true);

    let ui_weak: Weak<MainWindow> = app.as_weak();

    std::thread::spawn(move || {
        let interval = Duration::from_secs(3);

        loop {
            let online = check_online();

            let ui_weak2 = ui_weak.clone();
            let result = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    AppState::get(&ui).set_network_online(online);
                }
            });

            if result.is_err() {
                break;
            }

            std::thread::sleep(interval);
        }
    });
}
