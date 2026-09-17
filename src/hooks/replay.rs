//! Streaming JSONL replay, including legacy journals. Bounds apply to bytes read.
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

pub(super) async fn scan(
    path: &std::path::Path,
    after: u64,
    through: u64,
    cancelled: impl Fn() -> bool,
    mut visit: impl FnMut(HookEvent) -> bool,
) -> Result<(), String> {
    if cancelled() {
        return Err("Hook replay cancelled.".into());
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut input = BufReader::with_capacity(8192, file.take((MAX_REPLAY_BYTES + 1) as u64));
    let mut record = Vec::new();
    let mut total = 0usize;
    loop {
        if cancelled() {
            return Err("Hook replay cancelled.".into());
        }
        let bytes = input.fill_buf().await.map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            break;
        } // An incomplete live/crash tail is retried later.
        let count = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |n| n + 1);
        total += count;
        if total > MAX_REPLAY_BYTES {
            return Err("Hook journal exceeds the 16 MiB read limit.".into());
        }
        if record.len() + count > MAX_RECORD_BYTES {
            return Err("Hook journal record exceeds 64 KiB.".into());
        }
        record.extend_from_slice(&bytes[..count]);
        input.consume(count);
        if record.last() != Some(&b'\n') {
            continue;
        }
        if !record.iter().all(u8::is_ascii_whitespace) {
            let event: HookEvent = serde_json::from_slice(&record)
                .map_err(|e| format!("Invalid Hook journal record: {e}"))?;
            if event.seq > through {
                break;
            }
            if event.seq > after && !visit(event) {
                break;
            }
        }
        record.clear();
    }
    Ok(())
}

pub(super) async fn read_log(
    path: &std::path::Path,
    run: &mut HookRun,
    cancelled: impl Fn() -> bool,
) -> Result<String, String> {
    let recovered = run.conclusion == Some(HookConclusion::Interrupted);
    let through = if recovered { u64::MAX } else { run.seq };
    let mut last = run.seq;
    let mut text = String::new();
    scan(path, 0, through, cancelled, |event| {
        last = last.max(event.seq);
        append_line(&mut text, &event);
        true
    })
    .await?;
    // Recovered runs have no writer. Their durable summary may predate output
    // that reached the journal, so derive the display watermark from that file.
    if recovered {
        run.seq = last;
    }
    Ok(text)
}

fn append_line(text: &mut String, event: &HookEvent) {
    use std::fmt::Write;
    if event.kind == "output" {
        let _ = write!(
            text,
            "[{}] {}",
            event.data["stream"].as_str().unwrap_or("stdout"),
            event.data["text"].as_str().unwrap_or_default()
        );
    } else {
        let _ = writeln!(text, "[{}] {}", event.kind, event.data);
    }
}
