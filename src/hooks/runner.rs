use super::{HookService, state::Action, types::*};
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{mpsc, watch},
};

struct Journal {
    file: Option<tokio::fs::File>,
    bytes: usize,
    truncated: bool,
    tail: String,
    last_delivery: Instant,
    error: Option<String>,
}

impl Journal {
    async fn open(service: &HookService, run: &HookRun) -> Self {
        let result = async {
            tokio::fs::create_dir_all(&service.directory).await?;
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            options.mode(0o600);
            options
                .open(service.directory.join(format!("{}.jsonl", run.id)))
                .await
        }
        .await;
        let (file, error) = match result {
            Ok(file) => (Some(file), None),
            Err(e) => (None, Some(format!("Cannot create Hook log: {e}"))),
        };
        Self {
            file,
            bytes: 0,
            truncated: false,
            tail: String::new(),
            last_delivery: Instant::now(),
            error,
        }
    }

    async fn record(
        &mut self,
        service: &HookService,
        run: &mut HookRun,
        kind: &str,
        data: serde_json::Value,
    ) -> bool {
        let mut event = HookEvent {
            run_id: run.id.clone(),
            hook_id: run.hook_id.clone(),
            event_id: run.event_id.clone(),
            seq: run.seq + 1,
            time_ms: now_ms(),
            kind: kind.into(),
            data,
        };
        let mut json = serde_json::to_vec(&event).expect("serializable Hook event");
        json.push(b'\n');
        let limit = if kind == "output" {
            MAX_OUTPUT_BYTES
        } else {
            MAX_JOURNAL_BYTES
        };
        if self.bytes.saturating_add(json.len()) > limit || json.len() > MAX_RECORD_BYTES {
            if kind == "output" {
                return false;
            }
            self.error = Some("Hook lifecycle record exceeds journal budget.".into());
            self.file = None;
        }
        self.bytes += json.len();
        run.seq = event.seq;
        if let Some(file) = self.file.as_mut() {
            if let Err(e) = file.write_all(&json).await {
                self.error = Some(format!("Cannot write Hook log: {e}"));
                self.file = None;
            }
        }
        if kind == "hook.finished" {
            if let Some(file) = self.file.as_mut() {
                if let Err(error) = file.sync_data().await {
                    self.error = Some(format!("Cannot sync Hook log: {error}"));
                }
            }
            if let Some(error) = &self.error {
                if !run.detail.contains(error) {
                    run.detail.push_str(&format!("\n{error}"));
                }
                if run.conclusion == Some(HookConclusion::Success) {
                    run.conclusion = Some(HookConclusion::Failure);
                }
                event.data = serde_json::json!({"conclusion": run.conclusion, "detail": run.detail, "exit_code": run.exit_code, "signal": run.signal});
            }
        }
        let line = if kind == "output" {
            format!(
                "[{}] {}",
                event.data["stream"].as_str().unwrap_or("stdout"),
                event.data["text"].as_str().unwrap_or_default()
            )
        } else {
            format!("[{kind}] {}\n", event.data)
        };
        self.tail.push_str(&line);
        if !self.tail.ends_with('\n') {
            self.tail.push('\n');
        }
        trim_front(&mut self.tail, MAX_TAIL_BYTES);
        if kind != "output" || self.last_delivery.elapsed() >= Duration::from_millis(100) {
            run.log = Arc::new(self.tail.clone());
            let _ = service.state.dispatch(Action::Run(run.clone()));
            self.last_delivery = Instant::now();
        }
        service.publish(event, run);
        true
    }

    async fn output(
        &mut self,
        service: &HookService,
        run: &mut HookRun,
        stream: &str,
        text: String,
    ) {
        if self.truncated {
            return;
        }
        if !self
            .record(
                service,
                run,
                "output",
                serde_json::json!({"stream": stream, "text": text}),
            )
            .await
        {
            if !self.truncated {
                self.truncated = true;
                self.record(
                    service,
                    run,
                    "output.truncated",
                    serde_json::json!({"limit_bytes": MAX_OUTPUT_BYTES}),
                )
                .await;
            }
        }
    }

    async fn finish(
        &mut self,
        service: &HookService,
        run: &mut HookRun,
        conclusion: HookConclusion,
        detail: String,
    ) {
        run.conclusion = Some(conclusion);
        run.finished_at_ms = Some(now_ms());
        run.detail = detail;
        if let Some(file) = self.file.as_mut() {
            if let Err(e) = file.flush().await {
                self.error = Some(format!("Cannot flush Hook log: {e}"));
            }
        }
        if let Some(error) = &self.error {
            run.detail.push_str(&format!("\n{error}"));
            if conclusion == HookConclusion::Success {
                run.conclusion = Some(HookConclusion::Failure);
            }
        }
        self.record(
            service,
            run,
            "hook.finished",
            serde_json::json!({"conclusion": run.conclusion, "detail": run.detail,
            "exit_code": run.exit_code, "signal": run.signal}),
        )
        .await;
    }
}

pub(super) async fn benchmark_journal(service: &HookService) -> HookRun {
    let hook = HookDefinition {
        name: "P6 journal".into(),
        script: "true".into(),
        ..Default::default()
    };
    let trigger = HookTrigger::new(PR_REVIEW_APPROVED, "fixture", serde_json::json!({}));
    let mut run = HookRun::queued(&hook, &trigger);
    run.started_at_ms = Some(now_ms());
    let mut journal = Journal::open(service, &run).await;
    journal
        .record(
            service,
            &mut run,
            "execution.started",
            serde_json::json!({}),
        )
        .await;
    for _ in 0..9000 {
        journal
            .output(service, &mut run, "stdout", "x".into())
            .await;
    }
    for _ in 0..1024 {
        journal
            .output(service, &mut run, "stderr", "\u{0001}".repeat(1024))
            .await;
    }
    journal
        .finish(service, &mut run, HookConclusion::Success, String::new())
        .await;
    let bytes = tokio::fs::metadata(service.directory.join(format!("{}.jsonl", run.id)))
        .await
        .unwrap()
        .len();
    log::info!(
        "[P6][{}] journal_bytes={bytes}",
        std::env::var("USHER_P6_BENCHMARK").unwrap_or_default()
    );
    run
}

#[cfg(test)]
mod journal_tests {
    use super::*;

    #[tokio::test]
    async fn escaped_output_leaves_room_for_failure_details_and_terminal_record() {
        let (_dir, service) = super::super::tests::service();
        let hook = HookDefinition::default();
        let trigger = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
        let mut run = HookRun::queued(&hook, &trigger);
        let mut journal = Journal::open(&service, &run).await;
        for _ in 0..600 {
            journal
                .output(&service, &mut run, "stderr", "\u{0000}".repeat(4096))
                .await;
        }
        journal
            .finish(
                &service,
                &mut run,
                HookConclusion::Failure,
                "\u{0000}".repeat(8192),
            )
            .await;
        let path = service.directory.join(format!("{}.jsonl", run.id));
        let bytes = tokio::fs::metadata(&path).await.unwrap().len() as usize;
        assert_eq!(bytes, journal.bytes);
        assert!(bytes <= MAX_JOURNAL_BYTES);
        let mut last = String::new();
        super::super::replay::scan(
            &path,
            0,
            u64::MAX,
            || false,
            |event| {
                last = event.kind;
                true
            },
        )
        .await
        .unwrap();
        assert_eq!(last, "hook.finished");
        assert_eq!(run.conclusion, Some(HookConclusion::Failure));
        service.shutdown().await;
    }
}

fn trim_front(text: &mut String, max: usize) {
    if text.len() > max {
        let mut cut = text.len() - max;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text.drain(..cut);
    }
}

async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

pub(super) async fn run(
    service: Arc<HookService>,
    hook: HookDefinition,
    trigger: HookTrigger,
    mut run: HookRun,
    mut cancel: watch::Receiver<bool>,
) {
    let mut journal = Journal::open(&service, &run).await;
    journal.record(&service, &mut run, "hook.triggered", serde_json::json!({"event": trigger.name, "test": trigger.test, "revision": hook.revision})).await;
    journal
        .record(&service, &mut run, "hook.queued", serde_json::json!({}))
        .await;
    if let Some(error) = journal.error.clone() {
        journal
            .finish(&service, &mut run, HookConclusion::StartFailed, error)
            .await;
        return;
    }
    let project = service.project_lock(trigger.project.as_deref().unwrap_or("application"));
    let permits = async { (project.lock().await, service.slots.acquire().await) };
    let _permits = tokio::select! {
        permits = permits => permits,
        _ = cancelled(&mut cancel) => {
            journal.finish(&service, &mut run, HookConclusion::Cancelled, "Cancelled while queued.".into()).await;
            return;
        }
    };
    if !trigger.test {
        if !service.project_allows(&trigger, &hook.id) {
            journal
                .finish(
                    &service,
                    &mut run,
                    HookConclusion::Skipped,
                    "Hook is no longer selected by this project.".into(),
                )
                .await;
            return;
        }
        let state = service.state.snapshot();
        if !state.state().enabled
            || !state
                .state()
                .hooks
                .iter()
                .any(|h| h.id == hook.id && h.enabled)
        {
            journal
                .finish(
                    &service,
                    &mut run,
                    HookConclusion::Skipped,
                    "Hook was disabled or deleted while queued.".into(),
                )
                .await;
            return;
        }
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(hook.timeout_seconds);
    let prepared = tokio::select! {
        prepared = prepare(&hook, &trigger, &run.id) => prepared,
        _ = cancelled(&mut cancel) => {
            journal.finish(&service, &mut run, HookConclusion::Cancelled, "Cancelled during preparation.".into()).await;
            return;
        }
        _ = tokio::time::sleep_until(deadline) => {
            journal.finish(&service, &mut run, HookConclusion::TimedOut, "Hook preparation exceeded its timeout.".into()).await;
            return;
        }
    };
    let (mut command, _temporary) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            journal
                .finish(&service, &mut run, HookConclusion::StartFailed, error)
                .await;
            return;
        }
    };
    if !trigger.test {
        let state = service.state.snapshot();
        if !state.state().enabled
            || !state
                .state()
                .hooks
                .iter()
                .any(|h| h.id == hook.id && h.enabled)
        {
            journal
                .finish(
                    &service,
                    &mut run,
                    HookConclusion::Skipped,
                    "Hook was disabled before execution.".into(),
                )
                .await;
            return;
        }
    }
    if *cancel.borrow() {
        journal
            .finish(
                &service,
                &mut run,
                HookConclusion::Cancelled,
                "Cancelled before execution.".into(),
            )
            .await;
        return;
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            journal.finish(&service, &mut run, HookConclusion::StartFailed,
                format!("Cannot start {} interpreter '{}': {error}. Check its executable path, permissions, and working directory.", hook.language.label(), interpreter(&hook))).await;
            return;
        }
    };
    let pid = child.id().expect("spawned child PID");
    let tree = match super::process_tree::ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill().await;
            journal
                .finish(
                    &service,
                    &mut run,
                    HookConclusion::StartFailed,
                    format!("Cannot establish process-tree cancellation: {error}"),
                )
                .await;
            return;
        }
    };
    let started = Instant::now();
    run.started_at_ms = Some(now_ms());
    journal.record(&service, &mut run, "execution.started", serde_json::json!({"pid": pid, "language": hook.language.label(), "interpreter": interpreter(&hook)})).await;
    let (sender, mut output) = mpsc::channel(32);
    let stdout = tokio::spawn(read_output(
        child.stdout.take().unwrap(),
        sender.clone(),
        "stdout",
    ));
    let stderr = tokio::spawn(read_output(child.stderr.take().unwrap(), sender, "stderr"));
    #[cfg(windows)]
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"run\n").await;
    }
    let timeout = tokio::time::sleep_until(deadline);
    tokio::pin!(timeout);
    let mut stderr_tail = String::new();
    let mut readers_open = true;
    let (mut conclusion, mut detail) = loop {
        tokio::select! {
            _ = cancelled(&mut cancel) => { break (HookConclusion::Cancelled, "Execution cancelled.".into()); }
            _ = &mut timeout => { break (HookConclusion::TimedOut, format!("Execution exceeded the {} second timeout.", hook.timeout_seconds)); }
            result = child.wait() => {
                match result {
                    Ok(status) => {
                        run.exit_code = status.code();
                        #[cfg(unix)] { use std::os::unix::process::ExitStatusExt; run.signal = status.signal(); }
                        break if status.success() { (HookConclusion::Success, "Execution completed successfully.".into()) }
                            else { (HookConclusion::Failure, format!("Interpreter exited with {status}.")) };
                    }
                    Err(error) => break (HookConclusion::Failure, format!("Cannot wait for interpreter: {error}")),
                }
            }
            chunk = output.recv(), if readers_open => {
                match chunk {
                    Some((stream, text)) => {
                        if stream == "stderr" { stderr_tail.push_str(&text); trim_front(&mut stderr_tail, 8192); }
                        journal.output(&service, &mut run, stream, text).await;
                    }
                    None => readers_open = false,
                }
            }
        }
    };
    tree.terminate();
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    // Descendants must not keep the pipes or run alive after the interpreter exits.
    let drain = async {
        while let Some((stream, text)) = output.recv().await {
            if stream == "stderr" {
                stderr_tail.push_str(&text);
                trim_front(&mut stderr_tail, 8192);
            }
            journal.output(&service, &mut run, stream, text).await;
        }
    };
    if tokio::time::timeout(Duration::from_secs(2), drain)
        .await
        .is_err()
    {
        detail.push_str(" Output drain timed out; a detached process may still hold a pipe.");
        if conclusion == HookConclusion::Success {
            conclusion = HookConclusion::Failure;
        }
    }
    stdout.abort();
    stderr.abort();
    let _ = stdout.await;
    let _ = stderr.await;
    if conclusion != HookConclusion::Success && !stderr_tail.trim().is_empty() {
        detail.push_str("\n");
        detail.push_str(stderr_tail.trim());
    }
    let finished = serde_json::json!({"conclusion": conclusion, "exit_code": run.exit_code,
        "signal": run.signal, "duration_ms": started.elapsed().as_millis()});
    journal
        .record(&service, &mut run, "execution.finished", finished)
        .await;
    journal.finish(&service, &mut run, conclusion, detail).await;
}

async fn read_output(
    mut reader: impl AsyncRead + Unpin,
    sender: mpsc::Sender<(&'static str, String)>,
    stream: &'static str,
) {
    let mut bytes = [0u8; 4096];
    let mut pending = Vec::new();
    loop {
        match reader.read(&mut bytes).await {
            Ok(0) => break,
            Ok(count) => {
                pending.extend_from_slice(&bytes[..count]);
                let valid = match std::str::from_utf8(&pending) {
                    Ok(_) => pending.len(),
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    Err(_) => pending.len(),
                };
                if valid > 0 {
                    let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                    pending.drain(..valid);
                    if sender.send((stream, text)).await.is_err() {
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = sender
                    .send(("stderr", format!("Cannot read {stream}: {error}")))
                    .await;
                break;
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender
            .send((stream, String::from_utf8_lossy(&pending).into_owned()))
            .await;
    }
}

fn interpreter(hook: &HookDefinition) -> &str {
    if !hook.interpreter.trim().is_empty() {
        return hook.interpreter.trim();
    }
    match hook.language {
        HookLanguage::Shell => {
            if cfg!(windows) {
                "powershell.exe"
            } else {
                "/bin/sh"
            }
        }
        _ => "node",
    }
}

async fn prepare(
    hook: &HookDefinition,
    trigger: &HookTrigger,
    run_id: &str,
) -> Result<(Command, tempfile::TempDir), String> {
    hook.validate()?;
    let temporary =
        tokio::task::spawn_blocking(|| tempfile::Builder::new().prefix("usher-hook-").tempdir())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Cannot create Hook working files: {e}"))?;
    let extension = match hook.language {
        HookLanguage::Shell => {
            if cfg!(windows) {
                "ps1"
            } else {
                "sh"
            }
        }
        HookLanguage::JavaScriptTypeScript => "mts",
    };
    let script = temporary.path().join(format!("hook.{extension}"));
    let context = temporary.path().join("context.json");
    tokio::fs::write(&script, &hook.script)
        .await
        .map_err(|e| format!("Cannot write Hook script: {e}"))?;
    tokio::fs::write(
        &context,
        serde_json::to_vec_pretty(trigger).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("Cannot write Hook context: {e}"))?;
    let cwd = trigger
        .cwd
        .as_ref()
        .unwrap_or(&temporary.path().to_path_buf())
        .to_path_buf();
    let metadata = tokio::fs::metadata(&cwd)
        .await
        .map_err(|e| format!("Working directory '{}': {e}", cwd.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "Working directory '{}' is not a directory.",
            cwd.display()
        ));
    }
    let mut command = Command::new(interpreter(hook));
    #[cfg(windows)]
    let script = {
        let bootstrap = temporary
            .path()
            .join(if hook.language == HookLanguage::Shell {
                "bootstrap.ps1"
            } else {
                "bootstrap.mjs"
            });
        let source = if hook.language == HookLanguage::Shell {
            "$ErrorActionPreference = 'Stop'\n$PSNativeCommandUseErrorActionPreference = $true\nif ([Console]::ReadLine() -ne 'run') { exit 1 }\n$global:LASTEXITCODE = 0\n& $env:USHER_HOOK_SCRIPT\nif ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }\n"
        } else {
            "import { createInterface } from 'node:readline';\nimport { pathToFileURL } from 'node:url';\nconst gate = createInterface({ input: process.stdin });\nawait new Promise(resolve => gate.once('line', resolve));\ngate.close();\nawait import(pathToFileURL(process.env.USHER_HOOK_SCRIPT).href);\n"
        };
        tokio::fs::write(&bootstrap, source)
            .await
            .map_err(|e| format!("Cannot write Hook bootstrap: {e}"))?;
        command.env("USHER_HOOK_SCRIPT", &script);
        bootstrap
    };
    match hook.language {
        HookLanguage::Shell => {
            #[cfg(windows)]
            command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"]);
            #[cfg(not(windows))]
            command.arg("-e");
        }
        _ => {
            command.arg("--");
        }
    }
    command
        .arg(script)
        .current_dir(cwd)
        .env("USHER_HOOK_CONTEXT", context)
        .env("USHER_HOOK_RUN_ID", run_id)
        .env("USHER_HOOK_TEST", if trigger.test { "1" } else { "0" })
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x08000000).stdin(Stdio::piped()); // CREATE_NO_WINDOW
    Ok((command, temporary))
}
