#[cfg(not(unix))]
use super::io::IO_RETRY;
use super::io::InputQueue;
use crossbeam_channel::{Receiver, Sender, bounded};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::Read;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

pub(super) const OUTPUT_CHUNKS: usize = 64;
const OUTPUT_CHUNK_BYTES: usize = 8192;

pub enum SessionEvent {
    Output(Vec<u8>),
    WriteFailed,
    Exit,
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    input: InputQueue,
    cancelled: Arc<AtomicBool>,
    cancel_tx: Option<Sender<()>>,
    reader_stopped: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    #[cfg(unix)]
    process_id: Option<u32>,
}

impl PtySession {
    pub fn spawn(
        cwd: &str,
        cols: u16,
        rows: u16,
        tx: Sender<SessionEvent>,
    ) -> Result<Self, String> {
        let shell = default_shell();
        let mut cmd = CommandBuilder::new(shell.program.clone());
        apply_shell_mode_args(&mut cmd, &shell.program);
        if !cwd.trim().is_empty() {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        Self::spawn_command(cmd, cols, rows, tx)
    }

    fn spawn_command(
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        tx: Sender<SessionEvent>,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("openpty failed: {error}"))?;

        #[cfg(unix)]
        let reader_fd = {
            let fd = pair
                .master
                .as_raw_fd()
                .ok_or("PTY master has no file descriptor")?;
            // Reader and writer clones share the master's open-file flags.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(format!(
                    "set nonblocking PTY failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            fd
        };
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("clone reader failed: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("take writer failed: {error}"))?;
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| format!("spawn shell failed: {error}"))?;
        #[cfg(unix)]
        let process_id = child.process_id();
        drop(pair.slave);
        let cancelled = Arc::new(AtomicBool::new(false));
        let (cancel_tx, cancel_rx) = bounded::<()>(0);
        let reader_cancel_rx = cancel_rx.clone();
        let reader_stopped = Arc::new(AtomicBool::new(false));
        let reader_stop = reader_stopped.clone();
        let reader_cancelled = cancelled.clone();
        let reader_tx = tx.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; OUTPUT_CHUNK_BYTES];
            while !reader_stop.load(Ordering::Acquire) {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        send_event(
                            &reader_tx,
                            SessionEvent::Exit,
                            &reader_cancelled,
                            &reader_cancel_rx,
                        );
                        break;
                    }
                    Ok(n) => {
                        // During teardown, drain the kernel PTY without publishing.
                        // macOS child exit can wait for pending terminal output.
                        if reader_cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        if !send_event(
                            &reader_tx,
                            SessionEvent::Output(buf[..n].to_vec()),
                            &reader_cancelled,
                            &reader_cancel_rx,
                        ) && !reader_cancelled.load(Ordering::Acquire)
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        #[cfg(unix)]
                        {
                            // The master remains owned until this worker is joined.
                            let mut descriptor = libc::pollfd {
                                fd: reader_fd,
                                events: libc::POLLIN,
                                revents: 0,
                            };
                            unsafe {
                                libc::poll(&mut descriptor, 1, 100);
                            }
                        }
                        #[cfg(not(unix))]
                        std::thread::sleep(IO_RETRY);
                    }
                    Err(_) => {
                        send_event(
                            &reader_tx,
                            SessionEvent::Exit,
                            &reader_cancelled,
                            &reader_cancel_rx,
                        );
                        break;
                    }
                }
            }
        });
        let input = InputQueue::default();
        let writer_input = input.clone();
        let writer_cancelled = cancelled.clone();
        let writer = std::thread::spawn(move || {
            if let Err(error) = writer_input.write_to(writer) {
                writer_input.close();
                if !writer_cancelled.load(Ordering::Acquire) {
                    log::error!("[Terminal] PTY writer failed: {error}");
                    send_event(
                        &tx,
                        SessionEvent::WriteFailed,
                        &writer_cancelled,
                        &cancel_rx,
                    );
                }
            }
        });

        Ok(Self {
            master: pair.master,
            input,
            cancelled,
            cancel_tx: Some(cancel_tx),
            reader_stopped,
            workers: vec![reader, writer],
            child,
            #[cfg(unix)]
            process_id,
        })
    }

    pub fn enqueue_input(&self, bytes: &[u8]) -> Result<(), String> {
        self.input.enqueue(bytes)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("resize failed: {error}"))
    }

    pub fn terminate(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_tx.take();
        self.input.close();
        self.terminate_child();
        self.reader_stopped.store(true, Ordering::Release);
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // Cancellation can race entry into a synchronous Windows pipe call.
            // Repeat until both workers have observed shutdown and exited.
            while self.workers.iter().any(|worker| !worker.is_finished()) {
                for worker in &self.workers {
                    unsafe {
                        windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                    }
                }
                std::thread::sleep(IO_RETRY);
            }
        }
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                log::warn!("[Terminal] PTY I/O worker panicked");
            }
        }
    }

    fn terminate_child(&mut self) {
        #[cfg(unix)]
        self.signal_process_tree(libc::SIGTERM);
        #[cfg(not(unix))]
        let _ = self.child.kill();

        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    log::warn!("[Terminal] Failed to poll PTY child during shutdown: {error}");
                    break;
                }
            }
        }

        #[cfg(unix)]
        self.signal_process_tree(libc::SIGKILL);
        let _ = self.child.kill();
        if let Err(error) = self.child.wait() {
            log::warn!("[Terminal] Failed to reap PTY child: {error}");
        }
    }

    #[cfg(unix)]
    fn signal_process_tree(&mut self, signal: libc::c_int) {
        let Some(process_id) = self
            .process_id
            .and_then(|id| libc::pid_t::try_from(id).ok())
        else {
            let _ = self.child.kill();
            return;
        };
        // portable-pty starts the shell as the PTY session leader. Validate
        // that assumption before addressing a process group so Usher can
        // never signal its own group if a backend changes behavior.
        let process_group = unsafe { libc::getpgid(process_id) };
        if process_group == process_id {
            if unsafe { libc::killpg(process_group, signal) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    log::warn!(
                        "[Terminal] Failed to signal PTY process group {process_group}: {error}"
                    );
                }
            }
        } else {
            let _ = self.child.kill();
        }
    }
}

fn send_event(
    tx: &Sender<SessionEvent>,
    event: SessionEvent,
    cancelled: &AtomicBool,
    cancel_rx: &Receiver<()>,
) -> bool {
    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    // Disconnect broadcasts cancellation to every producer, including a full queue.
    crossbeam_channel::select_biased! {
        recv(cancel_rx) -> _ => false,
        send(tx, event) -> result => result.is_ok(),
    }
}

struct ShellProgram {
    program: String,
}

fn default_shell() -> ShellProgram {
    #[cfg(target_os = "windows")]
    {
        ShellProgram {
            program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        ShellProgram {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()),
        }
    }
}

fn apply_shell_mode_args(cmd: &mut CommandBuilder, program: &str) {
    #[cfg(target_os = "windows")]
    {
        let program_name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if program_name == "powershell.exe" || program_name == "pwsh.exe" {
            cmd.arg("-NoLogo");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let program_name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        // Start the shell in login + interactive mode so rc/profile files match
        // the user's normal terminal session as closely as possible.
        if matches!(program_name.as_str(), "zsh" | "bash" | "fish" | "sh") {
            if program_name == "fish" {
                cmd.arg("-i");
            } else {
                cmd.args(["-l", "-i"]);
            }
        }
    }
}
