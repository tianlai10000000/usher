//! Bounded process output transport and cancellation-safe drain.
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;

pub(super) const MAX_STEP_LOG_BYTES: usize = 32 * 1024;
const CHUNK_BYTES: usize = 4096;
const QUEUE_CHUNKS: usize = 16;
const TRUNCATED: &str = "[Truncated] Showing the last log bytes.\n";

pub(super) fn clamp_step_log(mut log: String) -> String {
    if log.len() <= MAX_STEP_LOG_BYTES {
        return log;
    }
    let mut start = log.len() - (MAX_STEP_LOG_BYTES - TRUNCATED.len());
    while !log.is_char_boundary(start) {
        start += 1;
    }
    let tail = log.split_off(start);
    let mut result = String::with_capacity(MAX_STEP_LOG_BYTES);
    result.push_str(TRUNCATED);
    result.push_str(&tail);
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputStream {
    Stdout,
    Stderr,
}

impl OutputStream {
    fn index(self) -> usize {
        match self {
            Self::Stdout => 0,
            Self::Stderr => 1,
        }
    }
}

enum Output {
    Bytes(OutputStream, Vec<u8>),
    End(OutputStream),
    Error(OutputStream, String),
}
async fn read_stream<R: AsyncRead + Unpin>(
    mut reader: R,
    stream: OutputStream,
    sender: mpsc::Sender<Output>,
) {
    let mut bytes = [0; CHUNK_BYTES];
    loop {
        match reader.read(&mut bytes).await {
            Ok(0) => {
                let _ = sender.send(Output::End(stream)).await;
                break;
            }
            Ok(count) => {
                if sender
                    .send(Output::Bytes(stream, bytes[..count].to_vec()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(error) => {
                let _ = sender
                    .send(Output::Error(
                        stream,
                        format!("error: failed to read process output: {error}\n"),
                    ))
                    .await;
                let _ = sender.send(Output::End(stream)).await;
                break;
            }
        }
    }
}

#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
    cr: bool,
}
impl Decoder {
    fn push(&mut self, bytes: &[u8], end: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut consumed = 0;
        while consumed < self.pending.len() {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(rest) => {
                    text.push_str(rest);
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&self.pending[consumed..consumed + valid]).unwrap(),
                    );
                    consumed += valid;
                    if let Some(len) = error.error_len() {
                        text.push('\u{fffd}');
                        consumed += len;
                    } else if end {
                        text.push('\u{fffd}');
                        consumed = self.pending.len();
                    } else {
                        break;
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        debug_assert!(self.pending.len() <= 3);
        let mut normalized = String::with_capacity(text.len() + 1);
        for character in text.chars() {
            if self.cr && character != '\n' {
                normalized.push('\r');
            }
            self.cr = character == '\r';
            if !self.cr {
                normalized.push(character);
            }
        }
        if end && self.cr {
            normalized.push('\r');
            self.cr = false;
        }
        normalized
    }
}
struct Readers(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Readers {
    fn drop(&mut self) {
        for handle in &self.0 {
            handle.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandFailureKind {
    EmptyCommand,
    Cancelled,
    Spawn,
    Wait,
    ReadStdout,
    ReadStderr,
    OutputConsumer,
    DrainTimeout,
    ExitStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandSuccess {
    pub(super) log: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandFailure {
    pub(super) kind: CommandFailureKind,
    pub(super) log: String,
}

impl CommandFailure {
    pub(super) fn is_cancelled(&self) -> bool {
        self.kind == CommandFailureKind::Cancelled
    }
}

pub(super) async fn run_command<F>(
    command: Vec<String>,
    cancel: &Arc<AtomicBool>,
    on_log: F,
) -> Result<CommandSuccess, CommandFailure>
where
    F: FnMut(String),
{
    run_command_with_stdout(command, cancel, on_log, |_| Ok(())).await
}

pub(super) async fn run_command_with_stdout<F, S>(
    command: Vec<String>,
    cancel: &Arc<AtomicBool>,
    mut on_log: F,
    mut on_stdout: S,
) -> Result<CommandSuccess, CommandFailure>
where
    F: FnMut(String),
    S: FnMut(&str) -> Result<(), String>,
{
    let Some(program) = command.first() else {
        return Err(CommandFailure {
            kind: CommandFailureKind::EmptyCommand,
            log: "Command vector is empty.".into(),
        });
    };
    let rendered = command.join(" ");
    let mut log = clamp_step_log(format!("Run {rendered}\n"));
    if cancel.load(Ordering::Acquire) {
        return Err(CommandFailure {
            kind: CommandFailureKind::Cancelled,
            log: format!("{log}Cancelled by user."),
        });
    }
    log::info!("[Workflow][Tag] Executing command: {rendered}");
    let mut child = Command::new(program)
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| CommandFailure {
            kind: CommandFailureKind::Spawn,
            log: format!("{log}error: failed to spawn command: {error}"),
        })?;
    let (sender, mut receiver) = mpsc::channel(QUEUE_CHUNKS);
    let mut readers = Readers(vec![
        tokio::spawn(read_stream(
            child.stdout.take().unwrap(),
            OutputStream::Stdout,
            sender.clone(),
        )),
        tokio::spawn(read_stream(
            child.stderr.take().unwrap(),
            OutputStream::Stderr,
            sender,
        )),
    ]);
    let mut decoders = [Decoder::default(), Decoder::default()];
    let mut status = None;
    let mut exited_at = None;
    let mut channel_closed = false;
    let mut pending = false;
    let mut last_emit = Instant::now();
    let mut peak_queue = 0;
    let mut received = 0usize;
    on_log(log.clone());
    let result: Result<(), CommandFailureKind> = loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.start_kill();
            let _ = child.wait().await;
            log.push_str("\nCancelled by user.");
            log = clamp_step_log(log);
            break Err(CommandFailureKind::Cancelled);
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(value)) => {
                    status = Some(value);
                    exited_at = Some(Instant::now());
                }
                Ok(None) => {}
                Err(error) => {
                    log.push_str(&format!("\nerror: failed to wait for command: {error}"));
                    break Err(CommandFailureKind::Wait);
                }
            }
        }
        if channel_closed && let Some(status) = status {
            break if status.success() {
                Ok(())
            } else {
                Err(CommandFailureKind::ExitStatus)
            };
        }
        // A descendant may keep inherited pipes open after the direct child exits.
        if exited_at.is_some_and(|time| time.elapsed() >= Duration::from_secs(2)) {
            log.push_str(
                "\nerror: output pipes remained open after process exit; stopped draining.",
            );
            log = clamp_step_log(log);
            break Err(CommandFailureKind::DrainTimeout);
        }
        tokio::select! {
            item = receiver.recv(), if !channel_closed => {
                peak_queue = peak_queue.max(receiver.len());
                let (stream, text) = match item {
                    Some(Output::Bytes(stream, bytes)) => { received += bytes.len(); (Some(stream), decoders[stream.index()].push(&bytes, false)) },
                    Some(Output::End(stream)) => (Some(stream), decoders[stream.index()].push(&[], true)),
                    Some(Output::Error(stream, error)) => {
                        log.push_str(&error);
                        log = clamp_step_log(log);
                        break Err(match stream {
                            OutputStream::Stdout => CommandFailureKind::ReadStdout,
                            OutputStream::Stderr => CommandFailureKind::ReadStderr,
                        });
                    }
                    None => { channel_closed = true; (None, String::new()) },
                };
                if stream == Some(OutputStream::Stdout)
                    && let Err(error) = on_stdout(&text)
                {
                    log.push_str(&format!("\nerror: failed to consume stdout: {error}"));
                    log = clamp_step_log(log);
                    break Err(CommandFailureKind::OutputConsumer);
                }
                if !text.is_empty() { log.push_str(&text); log = clamp_step_log(log); pending = true; }
            }
            _ = tokio::time::sleep(Duration::from_millis(40)) => {}
        }
        if pending && last_emit.elapsed() >= Duration::from_millis(50) {
            on_log(log.clone());
            last_emit = Instant::now();
            pending = false;
        }
    };
    if result.is_err() && status.is_none() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    // Cancel blocked sends before awaiting readers, on every exit path.
    for handle in &readers.0 {
        handle.abort();
    }
    for handle in readers.0.drain(..) {
        let _ = handle.await;
    }
    drop(receiver);
    log = clamp_step_log(log);
    on_log(log.clone());
    if std::env::var("USHER_P5_BENCHMARK").as_deref() == Ok("58") {
        log::info!(
            "[P5][58] transport received_bytes={received} peak_queue={peak_queue} queue_limit={QUEUE_CHUNKS} chunk_limit={CHUNK_BYTES} tail_bytes={}",
            log.len()
        );
    }
    match result {
        Ok(()) => Ok(CommandSuccess { log }),
        Err(kind) => Err(CommandFailure { kind, log }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_and_crlf_survive_every_chunk_boundary() {
        let input = "a中🦀\r\nb\r\nc".as_bytes();
        for cut in 0..=input.len() {
            let mut decoder = Decoder::default();
            let mut text = decoder.push(&input[..cut], false);
            text.push_str(&decoder.push(&input[cut..], true));
            assert_eq!(text, "a中🦀\nb\nc");
        }
        let mut decoder = Decoder::default();
        assert_eq!(decoder.push(&[0xff, 0xe4], true), "��");
        let tail = clamp_step_log("中".repeat(20000));
        assert!(tail.len() <= MAX_STEP_LOG_BYTES);
        assert!(tail.starts_with(TRUNCATED));
    }
    #[tokio::test]
    async fn reader_backpressure_retains_only_fixed_size_chunks() {
        let (sender, receiver) = mpsc::channel(QUEUE_CHUNKS);
        let task = tokio::spawn(read_stream(
            std::io::Cursor::new(vec![b'x'; 1024 * 1024]),
            OutputStream::Stdout,
            sender,
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(receiver.len(), QUEUE_CHUNKS);
        assert!(!task.is_finished());
        drop(receiver);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn drains_both_pipes_after_exit_and_cancels_busy_readers() {
        let token = Arc::new(AtomicBool::new(false));
        let output = tokio::time::timeout(Duration::from_secs(5), run_command(vec!["sh".into(), "-c".into(), "head -c 1048576 /dev/zero; printf '\\nTAIL-OUT\\n'; head -c 1048576 /dev/zero >&2; printf '\\nTAIL-ERR\\n' >&2".into()], &token, |_| {})).await.unwrap().unwrap();
        assert!(output.log.ends_with("TAIL-ERR\n"));
        assert!(output.log.len() <= MAX_STEP_LOG_BYTES);
        let trigger = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            trigger.store(true, Ordering::Release);
        });
        let output = tokio::time::timeout(
            Duration::from_secs(2),
            run_command(
                vec![
                    "sh".into(),
                    "-c".into(),
                    "while :; do printf 'x\\n'; done".into(),
                ],
                &token,
                |_| {},
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(output.kind, CommandFailureKind::Cancelled);
        assert!(output.log.ends_with("Cancelled by user."));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn distinguishes_spawn_exit_and_stdout_consumer_failures() {
        let token = Arc::new(AtomicBool::new(false));
        let spawn = run_command(vec!["/definitely/not/a/program".into()], &token, |_| {})
            .await
            .unwrap_err();
        assert_eq!(spawn.kind, CommandFailureKind::Spawn);

        let exit = run_command(
            vec!["sh".into(), "-c".into(), "exit 7".into()],
            &token,
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(exit.kind, CommandFailureKind::ExitStatus);

        let consumer = run_command_with_stdout(
            vec!["sh".into(), "-c".into(), "printf output".into()],
            &token,
            |_| {},
            |_| Err("fixture rejected output".into()),
        )
        .await
        .unwrap_err();
        assert_eq!(consumer.kind, CommandFailureKind::OutputConsumer);
        assert!(consumer.log.contains("fixture rejected output"));
    }
}
