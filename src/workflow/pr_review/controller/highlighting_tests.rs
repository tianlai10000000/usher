use super::super::*;
use super::{HighlightWorker, Request, compute};
use crate::workflow::syntax_highlight::{highlight_diff_lines, highlight_diff_lines_cancellable};
use std::sync::{LazyLock, mpsc};
use std::time::Duration;

static RESOURCES: LazyLock<Arc<HighlightResources>> =
    LazyLock::new(|| Arc::new(HighlightResources::load()));

fn file(path: &str, count: usize) -> SharedDiffFile {
    Arc::new(PullRequestDiffFile {
        path: path.into(),
        lines: (0..count)
            .map(|n| PullRequestDiffLine {
                old_line: (n + 1) as i32,
                new_line: (n + 1) as i32,
                kind: "context".into(),
                content: format!("\tlet value_{n}: u32 = 42; // 中文"),
            })
            .collect(),
        old_path: String::new(),
        change_type: "edit_file".into(),
        added_lines: 0,
        removed_lines: 0,
        viewed: false,
    })
}

fn request(
    state: &ReviewStateFacade,
    session: &HighlightSession<HighlightResources>,
    file: SharedDiffFile,
    index: usize,
    full_file: bool,
) -> Request {
    Request {
        state: state.clone(),
        generation: state.begin(ReviewRequestKind::Highlight),
        index,
        file,
        resources: session.lease(),
        cached_resources: Arc::default(),
        full_file,
        reset_scroll: full_file,
    }
}

#[test]
fn stale_queued_request_never_initializes_a_detached_or_current_slot() {
    let state = ReviewStateFacade::new();
    let session = HighlightSession::default();
    let old = request(&state, &session, file("same.rs", 8), 0, false);
    let old_slot = old.resources.clone();
    let old_cache = old.cached_resources.clone();
    session.detach();
    state.invalidate_all();
    assert!(compute(old, || false).is_none());
    assert!(old_slot.get().is_none());
    assert!(old_cache.get().is_none());
    assert!(!session.is_loaded());
    let current = request(&state, &session, file("same.rs", 8), 0, true);
    let new_slot = current.resources.clone();
    assert!(!Arc::ptr_eq(&old_slot, &new_slot));
    let completed = compute(current, || false).unwrap();
    assert_eq!(completed.highlighted.unwrap().len(), 8);
    assert!(new_slot.get().is_some());
    assert!(old_slot.get().is_none());
}

#[test]
fn cancellation_discards_partial_tokens_and_preserves_uncancelled_output() {
    let input = file("fixture.rs", 500);
    let mut polls = 0;
    let cancelled = highlight_diff_lines_cancellable(&RESOURCES, &input.path, &input.lines, || {
        polls += 1;
        polls > 2
    });
    assert!(cancelled.is_none());
    assert_eq!(polls, 3); // Admission, one processed line, then cancellation.
    let reference = highlight_diff_lines(&RESOURCES, &input.path, &input.lines);
    let complete =
        highlight_diff_lines_cancellable(&RESOURCES, &input.path, &input.lines, || false).unwrap();
    assert_eq!(reference, complete);
    assert_eq!(complete.len(), 500);
    assert_eq!(
        complete[0]
            .tokens
            .iter()
            .map(|t| t.text.as_str())
            .collect::<String>(),
        "    let value_0: u32 = 42; // 中文"
    );
}

#[test]
fn latest_selection_coalesces_pending_work_and_carries_mode_and_lease_identity() {
    let state = ReviewStateFacade::new();
    let session = HighlightSession::default();
    let owner = HighlightWorker::default();
    let worker = owner.engine().clone();
    let mut first = request(&state, &session, file("first.rs", 12000), 0, false);
    first.cached_resources = owner.resource_cache().lease();
    let first_cache = first.cached_resources.clone();
    let first_generation = first.generation;
    let first_slot = first.resources.clone();
    let (started, ready) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Arc::new(Mutex::new(gate));
    let (notify, notified) = mpsc::channel();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let observed = starts.clone();
    worker.submit(
        first,
        move |request, token| {
            observed.lock().unwrap().push(request.index);
            if request.generation == first_generation {
                started.send(()).unwrap();
                gate.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            compute(request, || token.cancelled())
        },
        move || {
            notify.send(()).unwrap();
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    owner.cancel();
    session.detach();
    assert!(owner.resource_cache().request_idle_check(None));
    drop(owner.resource_cache().confirm_idle().unwrap());
    assert!(owner.resource_cache().next_release().is_none());
    let mut obsolete_slots = Vec::new();
    for index in 1..33 {
        let mut next = request(
            &state,
            &session,
            file("same-path.rs", 5),
            index,
            index % 2 == 0,
        );
        next.cached_resources = owner.resource_cache().lease();
        obsolete_slots.push(Arc::downgrade(&next.resources));
        // Only the first active submit's closures drive the worker loop.
        // All request-specific identity must therefore remain in the input.
        worker.submit(
            next,
            |_, _| panic!("replacement closure must not run"),
            || {},
        );
    }
    release.send(()).unwrap();
    notified.recv_timeout(Duration::from_secs(5)).unwrap();
    let completion = worker.take().unwrap();
    assert_eq!(*starts.lock().unwrap(), vec![0, 32]);
    assert_eq!(completion.request.index, 32);
    assert_eq!(completion.request.file.path, "same-path.rs");
    assert!(completion.request.full_file);
    assert!(completion.request.reset_scroll);
    assert_eq!(completion.highlighted.unwrap().len(), 5);
    assert!(first_slot.get().is_none());
    assert!(first_cache.get().is_none());
    assert!(session.is_loaded());
    drop(completion.request);
    session.detach();
    assert!(obsolete_slots.iter().all(|slot| slot.upgrade().is_none()));
}

#[test]
fn cancel_removes_completed_output_before_a_cached_selection_or_reset() {
    let state = ReviewStateFacade::new();
    let session = HighlightSession::default();
    let owner = HighlightWorker::default();
    let worker = owner.engine().clone();
    let next = request(&state, &session, file("cached.rs", 10), 2, false);
    let (notify, ready) = mpsc::channel();
    worker.submit(
        next,
        |r, token| compute(r, || token.cancelled()),
        move || {
            notify.send(()).unwrap();
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    owner.cancel();
    assert!(worker.take().is_none());
    // Cancellation must not close the engine: same-project reentry still works.
    let next = request(&state, &session, file("cached.rs", 7), 2, true);
    let (notify, ready) = mpsc::channel();
    worker.submit(
        next,
        |r, token| compute(r, || token.cancelled()),
        move || {
            notify.send(()).unwrap();
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed = worker.take().unwrap();
    assert!(completed.request.full_file);
    assert_eq!(completed.highlighted.unwrap().len(), 7);
}

#[test]
fn grammar_reuse_retains_no_review_file_or_parser_state() {
    let state = ReviewStateFacade::new();
    let session = HighlightSession::default();
    let worker = HighlightWorker::default();
    let mut first_file = (*file("first.rs", 1)).clone();
    first_file.lines[0].content = "/* unclosed comment and USER_MARKER".into();
    let first_file = Arc::new(first_file);
    let file_weak = Arc::downgrade(&first_file);
    let mut first = request(&state, &session, first_file, 0, false);
    first.cached_resources = worker.resource_cache().lease();
    let completed = compute(first, || false).unwrap();
    let grammar = Arc::downgrade(completed.request.resources.get().unwrap());
    session.detach();
    drop(completed);
    assert!(file_weak.upgrade().is_none());
    assert!(grammar.upgrade().is_some()); // A slot detach alone does not request idle eviction.
    assert!(!session.is_loaded());
    assert!(worker.resource_cache().is_loaded());
    let fixtures: &[(&str, &[&str])] = &[
        ("second.rs", &["let fresh = Some(42);"]),
        ("script.py", &["def value():", "    return '新输入'"]),
        ("README.md", &["# fresh", "`code` stays separate"]),
        (
            "page.vue",
            &["<template>", "<div>fresh</div>", "</template>"],
        ),
        ("third.rs", &["let final_value = Some(7);"]),
    ];
    for (index, &(path, source)) in fixtures.iter().enumerate() {
        let mut next_file = (*file(path, source.len())).clone();
        for (line, content) in next_file.lines.iter_mut().zip(source) {
            line.content = (*content).into();
        }
        let next_file = Arc::new(next_file);
        let file_weak = Arc::downgrade(&next_file);
        let expected = highlight_diff_lines(
            &HighlightResources::load(),
            &next_file.path,
            &next_file.lines,
        );
        let mut next = request(&state, &session, next_file, index + 1, index % 2 == 1);
        next.cached_resources = worker.resource_cache().lease();
        let completed = compute(next, || false).unwrap();
        assert_eq!(completed.highlighted.as_ref().unwrap(), &expected);
        assert!(Arc::ptr_eq(
            completed.request.resources.get().unwrap(),
            &grammar.upgrade().unwrap(),
        ));
        session.detach();
        drop(completed);
        assert!(file_weak.upgrade().is_none());
    }
    let last_owner = worker.clone();
    drop(worker);
    assert!(grammar.upgrade().is_some());
    drop(last_owner);
    assert!(grammar.upgrade().is_none());
}
