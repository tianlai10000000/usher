use std::time::Duration;

use slint::{ComponentHandle, Global, Weak};

use crate::{AppState, MainWindow};

/// Check network connectivity using HTTP request (more reliable than raw TCP)
fn check_online() -> bool {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return false,
    };

    crate::logger::tracing_logger::install_thread_dispatch();

    rt.block_on(async {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .no_proxy()
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        // Try multiple connectivity check endpoints
        let urls = [
            "http://connectivitycheck.gstatic.com/generate_204",
            "http://captive.apple.com/hotspot-detect.html",
            "http://www.msftconnecttest.com/connecttest.txt",
        ];

        for url in urls {
            if let Ok(resp) = client.get(url).send().await {
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
    let initial = check_online();
    AppState::get(app).set_network_online(initial);

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
