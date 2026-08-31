use slint::{ComponentHandle, Global, Weak};

use crate::{AppState, MainWindow};

fn publish_network_state(ui_weak: &Weak<MainWindow>, online: bool) -> bool {
    let ui_weak = ui_weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            AppState::get(&ui).set_network_online(online);
        }
    })
    .is_ok()
}

pub fn setup_network_monitor(app: &MainWindow) {
    AppState::get(app).set_network_online(true);
    let ui_weak: Weak<MainWindow> = app.as_weak();

    setup_platform_monitor(ui_weak);
}

#[cfg(target_os = "macos")]
fn setup_platform_monitor(ui_weak: Weak<MainWindow>) {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use system_configuration::network_reachability::SCNetworkReachability;

    std::thread::spawn(move || {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
        let mut reachability = SCNetworkReachability::from(address);
        let online = reachability
            .reachability()
            .map(flags_are_online)
            .unwrap_or(true);
        if !publish_network_state(&ui_weak, online) {
            return;
        }

        let callback_ui = ui_weak.clone();
        if let Err(error) = reachability.set_callback(move |flags| {
            if !publish_network_state(&callback_ui, flags_are_online(flags)) {
                CFRunLoop::get_current().stop();
            }
        }) {
            log::warn!("[Network] Failed to register reachability callback: {error}");
            setup_backoff_monitor(ui_weak);
            return;
        }

        let run_loop = CFRunLoop::get_current();
        let scheduled =
            unsafe { reachability.schedule_with_runloop(&run_loop, kCFRunLoopDefaultMode) };
        if let Err(error) = scheduled {
            log::warn!("[Network] Failed to schedule reachability callback: {error}");
            setup_backoff_monitor(ui_weak);
            return;
        }
        CFRunLoop::run_current();
    });
}

#[cfg(target_os = "macos")]
fn flags_are_online(flags: system_configuration::network_reachability::ReachabilityFlags) -> bool {
    use system_configuration::network_reachability::ReachabilityFlags;

    flags.contains(ReachabilityFlags::REACHABLE)
        && (!flags.contains(ReachabilityFlags::CONNECTION_REQUIRED)
            || flags.intersects(
                ReachabilityFlags::CONNECTION_ON_DEMAND | ReachabilityFlags::CONNECTION_ON_TRAFFIC,
            ))
        && !flags.contains(ReachabilityFlags::INTERVENTION_REQUIRED)
}

#[cfg(not(target_os = "macos"))]
fn setup_platform_monitor(ui_weak: Weak<MainWindow>) {
    setup_backoff_monitor(ui_weak);
}

fn setup_backoff_monitor(ui_weak: Weak<MainWindow>) {
    use std::time::Duration;

    std::thread::spawn(move || {
        let mut retry_delay = Duration::from_secs(3);
        loop {
            let online = has_default_route();
            if !publish_network_state(&ui_weak, online) {
                break;
            }
            std::thread::sleep(if online {
                retry_delay = Duration::from_secs(3);
                Duration::from_secs(10)
            } else {
                let delay = retry_delay;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(300));
                delay
            });
        }
    });
}

fn has_default_route() -> bool {
    use std::net::UdpSocket;

    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("1.1.1.1:53")?;
            socket.local_addr()
        })
        .is_ok_and(|address| !address.ip().is_unspecified())
}
