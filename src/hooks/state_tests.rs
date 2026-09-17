use super::*;

#[test]
fn output_shares_configuration_skips_disk_and_terminal_state_survives_reload() {
    let (dir, service) = tests::service();
    for index in 0..32 {
        service
            .save(HookDefinition {
                id: format!("hook-{index}"),
                script: "#".repeat(MAX_SCRIPT_BYTES),
                ..Default::default()
            })
            .unwrap();
    }
    let before = service.state.snapshot();
    let mut run = HookRun::queued(
        &before.state().hooks[0],
        &HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({})),
    );
    run.started_at_ms = Some(now_ms());
    service.state.dispatch(Action::Run(run.clone())).unwrap();
    service.state.flush().unwrap();
    let path = dir.path().join("hooks.json");
    let persisted = std::fs::read(&path).unwrap();
    let metadata = std::fs::metadata(&path).unwrap().modified().unwrap();
    let durable = service.state.snapshot().state().durable_revision;
    for seq in 1..=100 {
        run.seq = seq;
        run.log = Arc::new(format!("output {seq}\n"));
        service.state.dispatch(Action::Run(run.clone())).unwrap();
        let state = service.state.snapshot();
        assert!(Arc::ptr_eq(&before.state().hooks, &state.state().hooks));
        assert!(Arc::ptr_eq(&before.state().events, &state.state().events));
        assert_eq!(state.state().durable_revision, durable);
    }
    service.state.flush().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), persisted);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        metadata
    );
    run.conclusion = Some(HookConclusion::Success);
    run.finished_at_ms = Some(now_ms());
    run.seq = 101;
    service.state.dispatch(Action::Run(run.clone())).unwrap();
    service.state.shutdown();
    let restored = HookStateFacade::new(path).unwrap();
    restored.hydrate().unwrap();
    let state = restored.snapshot();
    assert_eq!(state.state().hooks.len(), 32);
    assert_eq!(state.state().hooks[0].script.len(), MAX_SCRIPT_BYTES);
    let terminal = state.state().runs.iter().find(|r| r.id == run.id).unwrap();
    assert_eq!(terminal.seq, 101);
    assert_eq!(terminal.conclusion, Some(HookConclusion::Success));
    assert!(terminal.log.is_empty());
    restored.shutdown();
}

#[test]
fn editing_a_shared_catalog_preserves_older_snapshots_and_noops_do_not_commit() {
    let (_dir, service) = tests::service();
    let hook = HookDefinition {
        script: "echo original".into(),
        ..Default::default()
    };
    service.save(hook.clone()).unwrap();
    let before = service.state.snapshot();
    service
        .set_hook_enabled(hook.id.clone(), hook.enabled)
        .unwrap();
    assert_eq!(before.revision(), service.state.snapshot().revision());
    let mut edited = hook;
    edited.script = "echo edited".into();
    service.save(edited).unwrap();
    let after = service.state.snapshot();
    assert!(!Arc::ptr_eq(&before.state().hooks, &after.state().hooks));
    assert_eq!(before.state().hooks[0].script, "echo original");
    assert_eq!(after.state().hooks[0].script, "echo edited");
    assert!(after.state().catalog_revision > before.state().catalog_revision);
    service.state.shutdown();
}
