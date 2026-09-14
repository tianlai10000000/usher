//! One bounded effect driver for list logos and the edit-form preview.
//! Admission, retries and request identities remain in the Project Store.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::Notify;
use usher_state_store::Subscription;

use super::logo::{
    download_remote_logo_to_cache, find_cached_remote_logo_path, is_valid_logo_remote_url,
};
use super::logo_requests::MAX_LOGO_REQUESTS;
use super::state::ProjectStateFacade;
use crate::{MainWindow, app_runtime::shared_runtime};

pub(super) struct LogoLoader {
    task: tokio::task::JoinHandle<()>,
    stopped: Arc<AtomicBool>,
    subscription: Mutex<Option<Subscription>>,
    notify: Arc<Notify>,
    inputs: Arc<Mutex<PendingInputs>>,
}

#[derive(Default)]
struct PendingInputs {
    query: Option<String>,
    preview: Option<Option<String>>,
}

impl LogoLoader {
    pub(super) fn start(state: ProjectStateFacade, _window: slint::Weak<MainWindow>) -> Self {
        Self::start_with_completion(state, |_, _| {})
    }

    fn start_with_completion(
        state: ProjectStateFacade,
        completion: impl Fn(&str, bool) + Send + 'static,
    ) -> Self {
        let notify = Arc::new(Notify::new());
        let wake = notify.clone();
        let subscription = state.subscribe(Arc::new(move |_| wake.notify_one()));
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let inputs = Arc::new(Mutex::new(PendingInputs::default()));
        let pending_inputs = inputs.clone();
        let wake = notify.clone();
        let task = shared_runtime().spawn(async move {
            let mut active = FuturesUnordered::new();
            while !stop.load(Ordering::Acquire) {
                // UI only replaces these two pending inputs. Apply their typed
                // Store action on this worker, in order, without per-key tasks.
                let inputs =
                    std::mem::take(&mut *pending_inputs.lock().unwrap_or_else(|p| p.into_inner()));
                if inputs.query.is_some() || inputs.preview.is_some() {
                    state.update_logo_inputs(inputs.query, inputs.preview);
                }
                let sources = {
                    let snapshot = state.snapshot();
                    let query = &snapshot.state().logo_query;
                    snapshot
                        .state()
                        .projects
                        .iter()
                        .filter(|project| {
                            query.is_empty()
                                || project.project_name.to_lowercase().contains(query)
                                || project.project_alias.to_lowercase().contains(query)
                        })
                        .map(|project| project.logo.clone())
                        .chain(snapshot.state().preview_logo_url.iter().cloned())
                        .filter(|url| is_valid_logo_remote_url(url))
                        .collect()
                };
                state.sync_remote_logo_sources(sources);
                for request in state.claim_remote_logos(Instant::now()) {
                    active.push(async move {
                        let url = request.url.clone();
                        let cached = tokio::task::spawn_blocking(move || {
                            find_cached_remote_logo_path(&url).is_some()
                        })
                        .await
                        .unwrap_or(false);
                        let success =
                            cached || download_remote_logo_to_cache(&request.url).await.is_some();
                        (request, success)
                    });
                }
                // At capacity only completion can admit more work. An already
                // elapsed retry deadline must not turn a full pool into a spin.
                let deadline = if active.len() < MAX_LOGO_REQUESTS {
                    state.next_remote_logo_retry()
                } else {
                    None
                };
                tokio::select! {
                    _ = wake.notified() => {},
                    result = active.next(), if !active.is_empty() => {
                        if let Some((request, success)) = result
                            && !stop.load(Ordering::Acquire)
                        {
                            state.complete_remote_logo(&request, success, Instant::now());
                            completion(&request.url, success);
                        }
                    },
                    _ = async {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline.into()).await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    } => {},
                }
            }
            // Dropping the bounded futures cancels in-flight HTTP requests.
        });
        Self {
            task,
            stopped,
            subscription: Mutex::new(Some(subscription)),
            notify,
            inputs,
        }
    }

    pub(super) fn set_query(&self, query: &str) {
        self.inputs.lock().unwrap_or_else(|p| p.into_inner()).query = Some(query.to_owned());
        self.notify.notify_one();
    }

    pub(super) fn set_preview_url(&self, url: Option<String>) {
        self.inputs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .preview = Some(url);
        self.notify.notify_one();
    }

    pub(super) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.subscription
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        self.task.abort();
    }
}

impl Drop for LogoLoader {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{ProjectConfig, ProjectPrOverview};
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{atomic::AtomicUsize, mpsc};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn fixture(url: String) -> Vec<ProjectConfig> {
        vec![ProjectConfig {
            project_name: "fixture".into(),
            project_alias: "fixture".into(),
            local_repo: String::new(),
            remote_repo: String::new(),
            repo_project_id: String::new(),
            repo_type: "github".into(),
            logo: url,
            project_tags: Vec::new(),
            is_pr_auto_update: false,
            hook_ids: Vec::new(),
            pr_overview: ProjectPrOverview::default(),
        }]
    }

    #[test]
    fn stalled_http_times_out_then_shutdown_cancels_the_next_attempt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/stalled.png", listener.local_addr().unwrap());
        let (accepted, connections) = mpsc::channel();
        let (closed, disconnections) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "request never connected");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("accept failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(35)))
                    .unwrap();
                let mut input = [0u8; 4096];
                assert!(stream.read(&mut input).unwrap() > 0);
                accepted.send(()).unwrap();
                // Keep the server silent until the real shared-client timeout
                // or driver cancellation closes the socket. No mocked clock.
                loop {
                    match stream.read(&mut input) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                        Err(error) => panic!("client did not close stalled request: {error}"),
                    }
                }
                closed.send(()).unwrap();
            }
        });
        let state = ProjectStateFacade::memory(fixture(url));
        let (sent, received) = mpsc::channel();
        let loader = LogoLoader::start_with_completion(state.clone(), move |_, success| {
            let _ = sent.send(success);
        });
        connections.recv_timeout(Duration::from_secs(3)).unwrap();
        let started = Instant::now();
        assert!(!received.recv_timeout(Duration::from_secs(35)).unwrap());
        assert!(started.elapsed() >= Duration::from_secs(25));
        disconnections.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            connections
                .recv_timeout(Duration::from_millis(150))
                .is_err()
        );
        connections.recv_timeout(Duration::from_secs(3)).unwrap();
        loader.shutdown();
        disconnections.recv_timeout(Duration::from_secs(3)).unwrap();
        server.join().unwrap();
        assert!(received.recv_timeout(Duration::from_millis(150)).is_err());
        state.shutdown();
    }

    #[test]
    fn failed_http_logo_cools_down_and_explicit_reload_recovers_through_same_driver() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let url = format!("http://{}/p5-{nonce}.png", listener.local_addr().unwrap());
        let healthy = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicUsize::new(0));
        let server_healthy = healthy.clone();
        let server_stop = stop.clone();
        let requests = count.clone();
        let server = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut input = [0u8; 4096];
                        let _ = stream.read(&mut input);
                        requests.fetch_add(1, Ordering::AcqRel);
                        if server_healthy.load(Ordering::Acquire) {
                            let mut png = std::io::Cursor::new(Vec::new());
                            image::DynamicImage::new_rgba8(1, 1)
                                .write_to(&mut png, image::ImageFormat::Png)
                                .unwrap();
                            let png = png.into_inner();
                            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len()).unwrap();
                            stream.write_all(&png).unwrap();
                        } else {
                            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("local server failed: {error}"),
                }
            }
        });
        let projects = fixture(url.clone());
        let state = ProjectStateFacade::memory(projects.clone());
        let (sent, received) = mpsc::channel();
        let loader = LogoLoader::start_with_completion(state.clone(), move |_, success| {
            let _ = sent.send(success);
        });
        assert!(!received.recv_timeout(Duration::from_secs(3)).unwrap());
        assert!(received.recv_timeout(Duration::from_millis(150)).is_err());
        assert_eq!(count.load(Ordering::Acquire), 1);
        healthy.store(true, Ordering::Release);
        state.hydrate(projects);
        assert!(received.recv_timeout(Duration::from_secs(3)).unwrap());
        assert_eq!(count.load(Ordering::Acquire), 2);
        let cached = find_cached_remote_logo_path(&url).unwrap();
        assert!(image::load_from_memory(&std::fs::read(&cached).unwrap()).is_ok());
        loader.shutdown();
        state.shutdown();
        stop.store(true, Ordering::Release);
        server.join().unwrap();
        std::fs::remove_file(cached).unwrap();
    }
}
