use super::*;
use std::sync::mpsc;
use std::time::Duration;
use usher_messages::{MessageCenter, MessageDraft, NoopDesktopNotifier};

fn task(
    name: &'static str,
    priority: i32,
    order: u64,
    callback: impl Fn() + Send + Sync + 'static,
) -> CleanupTask {
    CleanupTask {
        name,
        priority,
        order,
        callback: Box::new(callback),
    }
}

#[test]
fn blocked_message_close_keeps_ui_thread_free_and_waits_for_last_producer() {
    let root_thread = std::thread::current().id();
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("messages.json");
    let (writing, written) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Arc::new(Mutex::new(blocked));
    // A real native delivery holds the final center close, without blocking
    // either the pre-shutdown caller or the independent UI work below.
    struct BlockedNotifier(mpsc::Sender<()>, Arc<Mutex<mpsc::Receiver<()>>>);
    impl usher_messages::DesktopNotifier for BlockedNotifier {
        fn show(&self, _: &usher_messages::Message) -> Result<(), String> {
            self.0.send(()).unwrap();
            self.1
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(())
        }
    }
    let center =
        MessageCenter::open_with(&path, Arc::new(BlockedNotifier(writing, blocked))).unwrap();
    let threads = Arc::new(Mutex::new(Vec::new()));
    let mut registry = CleanupRegistry::default();
    let publish = center.clone();
    registry
        .pre_shutdown
        .push(task("last_publisher", 100, 2, move || {
            assert_eq!(std::thread::current().id(), root_thread);
            publish
                .publish(MessageDraft::new("Last accepted message", "Fixture").desktop(true))
                .unwrap();
        }));
    let close = center.clone();
    let close_threads = threads.clone();
    registry
        .background_shutdown
        .push(task("message_center", 100, 1, move || {
            close_threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            close.shutdown_checked().unwrap();
        }));
    let ui_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ui = ui_count.clone();
    registry
        .ui_shutdown
        .push(task("native_ui", 100, 3, move || {
            assert_eq!(std::thread::current().id(), root_thread);
            ui.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
    let run = CleanupRun::new(registry);
    run.stop_producers();
    written.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        !run.finish(CleanupMode::EventLoop),
        "UI teardown must wait for the background barrier"
    );
    let background = run.clone();
    let waiter =
        crate::app_runtime::shared_runtime().spawn(async move { background.drain().await });
    // Simulate the event loop continuing to process independent UI work while
    // the real OS adapter is blocked. Completion cannot run before release.
    assert_eq!(ui_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    std::thread::sleep(Duration::from_millis(30));
    assert!(!waiter.is_finished());
    release.send(()).unwrap();
    crate::app_runtime::shared_runtime()
        .block_on(waiter)
        .unwrap();
    assert!(run.finish(CleanupMode::EventLoop));
    assert!(!run.finish(CleanupMode::EventLoop));
    assert_eq!(ui_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        threads
            .lock()
            .unwrap()
            .iter()
            .all(|thread| *thread != root_thread)
    );
    let restored = MessageCenter::open_with(path, Arc::new(NoopDesktopNotifier)).unwrap();
    assert_eq!(
        restored.snapshot().messages[0].title,
        "Last accepted message"
    );
    restored.shutdown_checked().unwrap();
}

#[test]
fn concurrent_and_cancelled_waiters_share_ordered_once_only_background_cleanup() {
    let history = Arc::new(Mutex::new(Vec::new()));
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Arc::new(Mutex::new(blocked));
    let mut registry = CleanupRegistry::default();
    for (name, priority, order) in [("low", 0, 1), ("high_old", 100, 2), ("high_new", 100, 3)] {
        let history = history.clone();
        let entered = entered.clone();
        let blocked = blocked.clone();
        registry
            .background_shutdown
            .push(task(name, priority, order, move || {
                history.lock().unwrap().push(name);
                if name == "high_new" {
                    entered.send(()).unwrap();
                    blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
            }));
    }
    let run = CleanupRun::new(registry);
    run.stop_producers();
    let first = run.clone();
    let waiting = crate::app_runtime::shared_runtime().spawn(async move { first.drain().await });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    waiting.abort();
    let second = run.clone();
    let next = crate::app_runtime::shared_runtime().spawn(async move { second.drain().await });
    std::thread::sleep(Duration::from_millis(30));
    assert!(!next.is_finished());
    release.send(()).unwrap();
    crate::app_runtime::shared_runtime().block_on(next).unwrap();
    crate::app_runtime::shared_runtime().block_on(run.drain());
    assert_eq!(
        *history.lock().unwrap(),
        vec!["high_new", "high_old", "low"]
    );
}

#[test]
fn panicking_background_callback_does_not_skip_tail_and_fallback_skips_native_ui() {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = CleanupRegistry::default();
    registry
        .background_shutdown
        .push(task("panic", 100, 1, || panic!("injected cleanup panic")));
    let ui_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ui_seen = ui_calls.clone();
    registry
        .ui_shutdown
        .push(task("ui_must_not_run", 100, 2, move || {
            ui_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
    let seen = count.clone();
    registry.post_shutdown.push(task("tail", 0, 3, move || {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let run = CleanupRun::new(registry);
    run.stop_producers();
    crate::app_runtime::shared_runtime().block_on(run.drain());
    assert!(run.finish(CleanupMode::BackgroundFallback));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(ui_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn concurrent_finish_waits_for_tail_and_finalizer_instead_of_treating_taken_as_complete() {
    let run = CleanupRun::new(CleanupRegistry::default());
    run.stop_producers();
    crate::app_runtime::shared_runtime().block_on(run.drain());
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let owner = run.clone();
    let first = std::thread::spawn(move || {
        owner.finish_with(CleanupMode::BackgroundFallback, || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let (finished, observed) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let owned = run.finish_with(CleanupMode::EventLoop, || panic!("duplicate finalizer"));
        finished.send(owned).unwrap();
    });
    assert!(observed.recv_timeout(Duration::from_millis(30)).is_err());
    release.send(()).unwrap();
    assert!(first.join().unwrap());
    assert!(!observed.recv_timeout(Duration::from_secs(5)).unwrap());
    second.join().unwrap();
}
