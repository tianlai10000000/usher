//! Session-owned bounded input and cancellable worker I/O.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub(super) const INPUT_BYTES: usize = 1024 * 1024;
const INPUT_ITEMS: usize = 128;
pub(super) const IO_RETRY: Duration = Duration::from_millis(2);

#[derive(Default)]
struct State {
    pending: VecDeque<Vec<u8>>,
    // Includes the buffer held by the writer until its entire write completes.
    bytes: usize,
    items: usize,
    closed: bool,
}

#[derive(Clone, Default)]
pub(super) struct InputQueue(Arc<(Mutex<State>, Condvar)>);

impl InputQueue {
    pub(super) fn enqueue(&self, bytes: &[u8]) -> Result<(), String> {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err("PTY input closed".into());
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > INPUT_BYTES.saturating_sub(state.bytes) || state.items >= INPUT_ITEMS {
            return Err("PTY input budget exceeded; entire input rejected".into());
        }
        state.pending.push_back(bytes.to_vec());
        state.bytes += bytes.len();
        state.items += 1;
        wake.notify_one();
        Ok(())
    }

    fn pop(&self) -> Option<Vec<u8>> {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.closed {
                return None;
            }
            if let Some(bytes) = state.pending.pop_front() {
                return Some(bytes);
            }
            state = wake.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn complete(&self, len: usize) {
        let mut state = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        if !state.closed {
            state.bytes -= len;
            state.items -= 1;
        }
    }

    pub(super) fn close(&self) {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.pending.clear();
        state.bytes = 0;
        state.items = 0;
        wake.notify_all();
    }

    pub(super) fn closed(&self) -> bool {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).closed
    }

    pub(super) fn write_to(&self, mut writer: impl Write) -> io::Result<()> {
        while let Some(bytes) = self.pop() {
            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() && !self.closed() {
                match writer.write(remaining) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(count) => remaining = &remaining[count..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(IO_RETRY);
                    }
                    Err(error) => return Err(error),
                }
            }
            while !self.closed() {
                match writer.flush() {
                    Ok(()) => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(IO_RETRY);
                    }
                    Err(error) => return Err(error),
                }
            }
            self.complete(bytes.len());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn admission_is_atomic_and_charges_inflight_bytes_and_items() {
        let queue = InputQueue::default();
        assert!(queue.enqueue(&vec![0; INPUT_BYTES + 1]).is_err());
        queue.enqueue(&vec![1; INPUT_BYTES]).unwrap();
        let inflight = queue.pop().unwrap();
        assert!(queue.enqueue(b"x").is_err());
        queue.complete(inflight.len());
        for _ in 0..INPUT_ITEMS {
            queue.enqueue(b"x").unwrap();
        }
        assert!(queue.enqueue(b"x").is_err());
        queue.close();
        assert!(queue.pop().is_none());
        assert!(queue.enqueue(b"x").is_err());
    }

    #[test]
    fn partial_writes_retry_without_reordering_or_duplication() {
        struct Partial {
            queue: InputQueue,
            bytes: Vec<u8>,
            calls: usize,
        }
        impl Write for Partial {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.calls += 1;
                if self.calls == 1 {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                if self.calls == 3 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                self.bytes.push(bytes[0]);
                if self.bytes.len() == 6 {
                    self.queue.close();
                }
                Ok(1)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let queue = InputQueue::default();
        queue.enqueue(b"abc").unwrap();
        queue.enqueue(b"def").unwrap();
        let mut writer = Partial {
            queue: queue.clone(),
            bytes: vec![],
            calls: 0,
        };
        queue.write_to(&mut writer).unwrap();
        assert_eq!(writer.bytes, b"abcdef");
    }

    #[test]
    fn close_wakes_idle_and_backpressured_writer() {
        struct Blocked(mpsc::Sender<()>);
        impl Write for Blocked {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                let _ = self.0.send(());
                Err(io::ErrorKind::WouldBlock.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        for busy in [false, true] {
            let queue = InputQueue::default();
            if busy {
                queue.enqueue(b"pending").unwrap();
            }
            let worker_queue = queue.clone();
            let (start, started) = mpsc::channel();
            let (done, finished) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                worker_queue.write_to(Blocked(start)).unwrap();
                done.send(()).unwrap();
            });
            if busy {
                started.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            queue.close();
            finished.recv_timeout(Duration::from_secs(2)).unwrap();
            worker.join().unwrap();
        }
    }
}
