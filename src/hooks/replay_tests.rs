use super::*;

#[tokio::test]
async fn journal_serialized_budget_preserves_truncation_and_terminal_records() {
    let (_dir, service) = tests::service();
    let run = runner::benchmark_journal(&service).await;
    let path = service.directory.join(format!("{}.jsonl", run.id));
    assert!(tokio::fs::metadata(&path).await.unwrap().len() <= MAX_JOURNAL_BYTES as u64);
    let mut kinds = Vec::new();
    let mut expected = 1;
    replay::scan(
        &path,
        0,
        u64::MAX,
        || false,
        |event| {
            assert_eq!(event.seq, expected);
            expected += 1;
            kinds.push(event.kind);
            true
        },
    )
    .await
    .unwrap();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| kind.as_str() == "output.truncated")
            .count(),
        1
    );
    assert_eq!(kinds.last().unwrap(), "hook.finished");
    let page = service.events_after(&run.id, 0).await.unwrap();
    assert_eq!(page.len(), 256);
    let next = service
        .events_after(&run.id, page.last().unwrap().seq)
        .await
        .unwrap();
    assert_eq!(next[0].seq, 257);
    service.shutdown().await;
}

fn event(seq: u64) -> HookEvent {
    HookEvent {
        run_id: "run-test".into(),
        hook_id: "hook-test".into(),
        event_id: "event-test".into(),
        seq,
        time_ms: 0,
        kind: "output".into(),
        data: serde_json::json!({"stream": "stdout", "text": "line\n"}),
    }
}

#[tokio::test]
async fn recovery_replays_durable_output_beyond_saved_summary_but_live_reads_keep_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovered.jsonl");
    let mut bytes = Vec::new();
    for seq in 1..=3 {
        let mut value = event(seq);
        value.data["text"] = format!("line-{seq}\n").into();
        serde_json::to_writer(&mut bytes, &value).unwrap();
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"{\"seq\":4");
    tokio::fs::write(&path, bytes).await.unwrap();
    let mut run = HookRun::queued(
        &HookDefinition::default(),
        &HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({})),
    );
    run.seq = 1;
    let live = replay::read_log(&path, &mut run, || false).await.unwrap();
    assert!(live.contains("line-1"));
    assert!(!live.contains("line-2"));
    assert_eq!(run.seq, 1);
    run.conclusion = Some(HookConclusion::Interrupted);
    let recovered = replay::read_log(&path, &mut run, || false).await.unwrap();
    assert!(recovered.contains("line-3"));
    assert_eq!(run.seq, 3);
    assert_eq!(run.conclusion, Some(HookConclusion::Interrupted));
}

#[tokio::test]
async fn replay_bounds_records_bytes_and_cancels_between_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.jsonl");
    let mut content = serde_json::to_vec(&event(1)).unwrap();
    content.push(b'\n');
    let record = content.clone();
    content.extend_from_slice(b"{\"seq\":2");
    tokio::fs::write(&path, &content).await.unwrap();
    let mut count = 0;
    replay::scan(
        &path,
        0,
        1,
        || false,
        |_| {
            count += 1;
            true
        },
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    let cancelled = AtomicBool::new(false);
    tokio::fs::write(&path, record.repeat(20)).await.unwrap();
    let error = replay::scan(
        &path,
        0,
        u64::MAX,
        || cancelled.load(Ordering::Acquire),
        |_| {
            cancelled.store(true, Ordering::Release);
            true
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("cancelled"));
    tokio::fs::write(&path, vec![b'x'; MAX_RECORD_BYTES + 1])
        .await
        .unwrap();
    assert!(
        replay::scan(&path, 0, u64::MAX, || false, |_| true)
            .await
            .unwrap_err()
            .contains("64 KiB")
    );
    // Newline-only bytes cannot hide behind a metadata check or grow a record buffer.
    tokio::fs::write(&path, vec![b'\n'; MAX_REPLAY_BYTES + 1])
        .await
        .unwrap();
    assert!(
        replay::scan(&path, 0, u64::MAX, || false, |_| true)
            .await
            .unwrap_err()
            .contains("16 MiB")
    );
    tokio::fs::write(&path, b"invalid\n").await.unwrap();
    assert!(
        replay::scan(&path, 0, u64::MAX, || false, |_| true)
            .await
            .is_err()
    );
}
