//! Opt-in offline response-budget fixture through the real Logo download path.
use crate::MainWindow;
use slint::ComponentHandle;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn setup(ui: &MainWindow) {
    if std::env::var("USHER_P6_BENCHMARK").as_deref() != Ok("67")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
            heartbeat(ui.as_weak(), Instant::now(), Instant::now());
            crate::app_runtime::shared_runtime().spawn(async {
                if let Err(error) = exercise().await {
                    log::error!("[P6][67] fixture_failed {error}");
                }
            });
        });
    });
}

fn heartbeat(weak: slint::Weak<MainWindow>, start: Instant, last: Instant) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if weak.upgrade().is_none() || start.elapsed() > Duration::from_secs(26) {
            return;
        }
        log::info!("[P6][67] heartbeat_us={}", last.elapsed().as_micros());
        heartbeat(weak, start, Instant::now());
    });
}

async fn download(large: bool) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/fixture.png", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0u8; 4096];
        stream.read(&mut request).await?;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await?;
        if large {
            let chunk = [b'x'; 64 * 1024];
            for _ in 0..1024 {
                stream.write_all(b"10000\r\n").await?;
                stream.write_all(&chunk).await?;
                stream.write_all(b"\r\n").await?;
                // Keep the live buffered transfer observable at 100 ms sampling.
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        } else {
            let png =
                include_bytes!("../../ui/assets/illustrations/vuetify-empty-state-teamwork.png");
            stream
                .write_all(format!("{:x}\r\n", png.len()).as_bytes())
                .await?;
            stream.write_all(png).await?;
            stream.write_all(b"\r\n").await?;
        }
        stream.write_all(b"0\r\n\r\n").await?;
        Ok::<_, std::io::Error>(())
    });
    let path = super::logo::download_remote_logo_to_cache(&url).await;
    // Early rejection closes the response; abort a still-blocked fixture writer.
    server.abort();
    let _ = server.await;
    if let Some(path) = &path {
        let bytes = tokio::fs::metadata(path).await?.len();
        log::info!("[P6][67] response large={large} accepted_bytes={bytes}");
        tokio::fs::remove_file(path).await?;
    } else {
        log::info!("[P6][67] response large={large} rejected");
    }
    Ok(path.is_some())
}

async fn exercise() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for cycle in 0..3 {
        let started = Instant::now();
        if !download(false).await? {
            return Err("initial small response failed".into());
        }
        let _ = download(true).await?;
        if !download(false).await? {
            return Err("small response after oversized response failed".into());
        }
        log::info!(
            "[P6][67] cycle={cycle} completed_ms={}",
            started.elapsed().as_millis()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    log::info!("[P6][67] complete");
    Ok(())
}
