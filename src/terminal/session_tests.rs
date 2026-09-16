use super::*;
use std::sync::mpsc;

#[test]
fn full_output_queue_is_lossless_and_cancellation_does_not_need_a_slot() {
    let (tx, rx) = bounded(1);
    let (cancel_tx, cancel_rx) = bounded(0);
    let cancelled = Arc::new(AtomicBool::new(false));
    assert!(send_event(
        &tx,
        SessionEvent::Output(vec![1]),
        &cancelled,
        &cancel_rx
    ));
    let stop = cancelled.clone();
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        assert!(send_event(
            &tx,
            SessionEvent::Output(vec![2]),
            &stop,
            &cancel_rx
        ));
        done.send(()).unwrap();
        assert!(!send_event(&tx, SessionEvent::Exit, &stop, &cancel_rx));
    });
    assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(matches!(rx.recv().unwrap(), SessionEvent::Output(bytes) if bytes == [1]));
    finished.recv_timeout(Duration::from_secs(2)).unwrap();
    cancelled.store(true, Ordering::Release);
    drop(cancel_tx);
    worker.join().unwrap();
    assert!(matches!(rx.recv().unwrap(), SessionEvent::Output(bytes) if bytes == [2]));
}

#[test]
fn disconnected_output_stops_production() {
    let (tx, rx) = bounded(1);
    let (_cancel_tx, cancel_rx) = bounded(0);
    drop(rx);
    assert!(!send_event(
        &tx,
        SessionEvent::Exit,
        &AtomicBool::new(false),
        &cancel_rx,
    ));
}

#[cfg(unix)]
fn shell(script: &str, tx: Sender<SessionEvent>) -> PtySession {
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    PtySession::spawn_command(command, 80, 24, tx).unwrap()
}

#[cfg(unix)]
#[test]
fn real_pty_backpressure_preserves_all_output_before_exit() {
    let (tx, rx) = bounded(2);
    let mut session = shell(
        "stty -onlcr; i=0; while [ $i -lt 20000 ]; do printf '0123456789\\n'; i=$((i+1)); done",
        tx,
    );
    std::thread::sleep(Duration::from_millis(50));
    let mut output = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            SessionEvent::Output(bytes) => {
                assert!(bytes.len() <= OUTPUT_CHUNK_BYTES);
                output.extend(bytes);
            }
            SessionEvent::Exit => break,
            SessionEvent::WriteFailed => panic!("unexpected input failure"),
        }
    }
    session.terminate();
    assert_eq!(output, b"0123456789\n".repeat(20000));
    assert!(session.workers.is_empty());
    assert!(session.child.try_wait().unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn real_pty_slow_stdin_and_full_output_shutdown_join_workers() {
    let (tx, rx) = bounded(1);
    let mut session = shell(
        "stty -echo -icanon; printf READY; while :; do printf 'output-output-output'; done",
        tx,
    );
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        SessionEvent::Output(_)
    ));
    let input = vec![b'x'; super::super::io::INPUT_BYTES];
    let started = Instant::now();
    session.enqueue_input(&input).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "admission blocked on the child"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        session.enqueue_input(&input).is_err(),
        "inflight input lost its budget"
    );
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        session.terminate();
        assert!(session.workers.is_empty());
        assert!(session.child.try_wait().unwrap().is_some());
        done.send(()).unwrap();
    });
    // Keep the full receiver alive: disconnect must not be required for shutdown.
    finished.recv_timeout(Duration::from_secs(3)).unwrap();
    worker.join().unwrap();
    drop(rx);
}
