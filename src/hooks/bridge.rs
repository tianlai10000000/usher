use super::projection::{ProjectionCache, RunProjection, project_run};
use super::*;
use crate::{MainWindow, Route, i18n, jobs};
use chrono::{DateTime, Local};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use usher_messages::{MessageCenter, MessageSeverity, NativeDelivery};
use usher_state_store::Subscription;
use usher_ui_settings::{
    GitHubSettingsPage, HookChoice, HookCodeLine, HookCodeToken, HookEventChoice,
    HookLanguageDraft, HookSettingsState, HookSummary,
};

static SELECTED_RUN: Mutex<String> = Mutex::new(String::new());
static LAST_PROJECTION: Mutex<Option<(String, u64, bool)>> = Mutex::new(None);

static UI_SUBSCRIPTION: Mutex<Option<Subscription>> = Mutex::new(None);
static REPLAY_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REPLAY_TASK: Mutex<Option<tokio::task::AbortHandle>> = Mutex::new(None);

pub(crate) fn cancel_replay() {
    REPLAY_GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Some(task) = REPLAY_TASK.lock().unwrap().take() {
        task.abort();
    }
}

const MAX_HOOK_HIGHLIGHT_BYTES: usize = 32 * 1024;
pub(super) const MAX_HOOK_HIGHLIGHT_LINES: usize = 256;
const MAX_HOOK_HIGHLIGHT_TOKENS: usize = 2_048;
const HOOK_SCRIPT_HYDRATION_DELAY: Duration = Duration::from_millis(24);
const HOOK_HIGHLIGHT_DELAY: Duration = Duration::from_millis(90);
pub(super) const HIGHLIGHT_STYLE_COMMENT: i32 = 1;
pub(super) const HIGHLIGHT_STYLE_KEYWORD: i32 = 2;
pub(super) const HIGHLIGHT_STYLE_STRING: i32 = 3;
pub(super) const HIGHLIGHT_STYLE_CONSTANT: i32 = 4;
pub(super) const HIGHLIGHT_STYLE_ENTITY: i32 = 5;
pub(super) const HIGHLIGHT_STYLE_VARIABLE: i32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StyledToken {
    text: String,
    style: i32,
}

fn push_styled_token(tokens: &mut Vec<StyledToken>, text: &str, style: i32) {
    if text.is_empty() {
        return;
    }
    if let Some(previous) = tokens.last_mut()
        && previous.style == style
    {
        previous.text.push_str(text);
    } else {
        tokens.push(StyledToken {
            text: text.to_owned(),
            style,
        });
    }
}

fn next_char_end(source: &str, start: usize) -> usize {
    start + source[start..].chars().next().map_or(0, char::len_utf8)
}

fn quoted_end(source: &str, start: usize, quote: u8) -> usize {
    let bytes = source.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 1;
            if index < bytes.len() {
                index = next_char_end(source, index);
            }
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index = next_char_end(source, index);
        }
    }
    bytes.len()
}

fn javascript_identifier_style(identifier: &str, entity_expected: bool) -> i32 {
    if entity_expected
        || identifier
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_uppercase)
    {
        return HIGHLIGHT_STYLE_ENTITY;
    }
    if matches!(
        identifier,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "from"
            | "function"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "let"
            | "new"
            | "of"
            | "return"
            | "switch"
            | "throw"
            | "try"
            | "type"
            | "typeof"
            | "var"
            | "while"
            | "with"
            | "yield"
            | "async"
            | "await"
            | "as"
            | "declare"
            | "keyof"
            | "namespace"
            | "private"
            | "protected"
            | "public"
            | "readonly"
            | "satisfies"
            | "static"
    ) {
        HIGHLIGHT_STYLE_KEYWORD
    } else if matches!(
        identifier,
        "true" | "false" | "null" | "undefined" | "NaN" | "Infinity"
    ) {
        HIGHLIGHT_STYLE_CONSTANT
    } else if matches!(
        identifier,
        "any"
            | "bigint"
            | "boolean"
            | "never"
            | "number"
            | "object"
            | "string"
            | "symbol"
            | "unknown"
            | "void"
    ) {
        HIGHLIGHT_STYLE_ENTITY
    } else if matches!(
        identifier,
        "console" | "fetch" | "JSON" | "process" | "require" | "module" | "exports"
    ) {
        HIGHLIGHT_STYLE_VARIABLE
    } else {
        0
    }
}

fn shell_identifier_style(identifier: &str, command_expected: bool) -> i32 {
    if matches!(
        identifier,
        "case"
            | "do"
            | "done"
            | "elif"
            | "else"
            | "esac"
            | "export"
            | "fi"
            | "for"
            | "function"
            | "if"
            | "in"
            | "local"
            | "readonly"
            | "return"
            | "select"
            | "then"
            | "until"
            | "while"
    ) {
        HIGHLIGHT_STYLE_KEYWORD
    } else if command_expected {
        HIGHLIGHT_STYLE_ENTITY
    } else {
        0
    }
}

fn tokenize_code_line(line: &str, language: i32, in_block_comment: &mut bool) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut entity_expected = false;
    let mut command_expected = true;

    while index < bytes.len() {
        if language == 1 && *in_block_comment {
            let end = line[index..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + offset + 2);
            push_styled_token(&mut tokens, &line[index..end], HIGHLIGHT_STYLE_COMMENT);
            index = end;
            if index == bytes.len() && !line.ends_with("*/") {
                break;
            }
            *in_block_comment = false;
            continue;
        }

        if language == 1 && line[index..].starts_with("//") {
            push_styled_token(&mut tokens, &line[index..], HIGHLIGHT_STYLE_COMMENT);
            break;
        }
        if language == 1 && line[index..].starts_with("/*") {
            if let Some(offset) = line[index + 2..].find("*/") {
                let end = index + 2 + offset + 2;
                push_styled_token(&mut tokens, &line[index..end], HIGHLIGHT_STYLE_COMMENT);
                index = end;
            } else {
                push_styled_token(&mut tokens, &line[index..], HIGHLIGHT_STYLE_COMMENT);
                *in_block_comment = true;
                break;
            }
            continue;
        }
        if language == 0
            && bytes[index] == b'#'
            && (index == 0 || bytes[index - 1].is_ascii_whitespace())
        {
            push_styled_token(&mut tokens, &line[index..], HIGHLIGHT_STYLE_COMMENT);
            break;
        }

        if matches!(bytes[index], b'\'' | b'\"' | b'`') {
            let end = quoted_end(line, index, bytes[index]);
            push_styled_token(&mut tokens, &line[index..end], HIGHLIGHT_STYLE_STRING);
            index = end;
            command_expected = false;
            continue;
        }

        if language == 0 && bytes[index] == b'$' {
            let mut end = index + 1;
            if end < bytes.len() && bytes[end] == b'{' {
                end += 1;
                while end < bytes.len() && bytes[end] != b'}' {
                    end = next_char_end(line, end);
                }
                end = (end + usize::from(end < bytes.len())).min(bytes.len());
            } else {
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
            }
            push_styled_token(&mut tokens, &line[index..end], HIGHLIGHT_STYLE_VARIABLE);
            index = end;
            command_expected = false;
            continue;
        }

        if bytes[index].is_ascii_digit() {
            let mut end = index + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric()
                    || matches!(bytes[end], b'.' | b'_' | b'x' | b'X'))
            {
                end += 1;
            }
            push_styled_token(&mut tokens, &line[index..end], HIGHLIGHT_STYLE_CONSTANT);
            index = end;
            command_expected = false;
            continue;
        }

        if bytes[index].is_ascii_alphabetic() || matches!(bytes[index], b'_' | b'$') {
            let mut end = index + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'$'))
            {
                end += 1;
            }
            let identifier = &line[index..end];
            let style = if language == 1 {
                javascript_identifier_style(identifier, entity_expected)
            } else {
                shell_identifier_style(identifier, command_expected)
            };
            push_styled_token(&mut tokens, identifier, style);
            entity_expected = language == 1
                && matches!(
                    identifier,
                    "class" | "enum" | "function" | "interface" | "namespace" | "type"
                );
            command_expected = false;
            index = end;
            continue;
        }

        let end = next_char_end(line, index);
        let segment = &line[index..end];
        push_styled_token(&mut tokens, segment, 0);
        if language == 0 && matches!(segment, ";" | "|") {
            command_expected = true;
        }
        index = end;
    }
    tokens
}

pub(super) fn hook_code_lines(source: &str, language: i32) -> Vec<HookCodeLine> {
    if source.is_empty() || source.len() > MAX_HOOK_HIGHLIGHT_BYTES {
        return Vec::new();
    }
    let source_lines = source.split('\n').collect::<Vec<_>>();
    if source_lines.len() > MAX_HOOK_HIGHLIGHT_LINES {
        return Vec::new();
    }

    let mut token_count = 0usize;
    let mut in_block_comment = false;
    let mut lines = Vec::with_capacity(source_lines.len());
    for line in source_lines {
        let styled = tokenize_code_line(line, language, &mut in_block_comment);
        token_count = token_count.saturating_add(styled.len());
        if token_count > MAX_HOOK_HIGHLIGHT_TOKENS {
            return Vec::new();
        }
        let mut tokens = Vec::with_capacity(styled.len());
        for token in styled {
            tokens.push(HookCodeToken {
                text: token.text.into(),
                style: token.style,
            });
        }
        lines.push(HookCodeLine {
            text: line.into(),
            tokens: ModelRc::new(VecModel::from(tokens)),
        });
    }
    lines
}

pub(super) fn hook_styled_code(
    source: &str,
    language: i32,
    colors: &[slint::Color; 6],
) -> Option<slint::StyledText> {
    use std::fmt::Write;

    let lines = hook_code_lines(source, language);
    if lines.is_empty() {
        return None;
    }
    let mut markup = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            markup.push_str("&#10;");
        }
        for token in line.tokens.iter() {
            let color = colors.get(token.style.wrapping_sub(1) as usize);
            if let Some(color) = color {
                write!(
                    markup,
                    "<font color='#{:02x}{:02x}{:02x}'>",
                    color.red(),
                    color.green(),
                    color.blue(),
                )
                .ok()?;
            }
            // Entities keep source whitespace and Markdown/HTML punctuation
            // literal without creating paragraphs, lists or code fences.
            for character in token.text.chars() {
                if character.is_ascii() && !character.is_ascii_alphanumeric() {
                    write!(markup, "&#{};", character as u32).ok()?;
                } else {
                    markup.push(character);
                }
            }
            if color.is_some() {
                markup.push_str("</font>");
            }
        }
    }
    let styled = slint::StyledText::from_markdown(&markup).ok()?;
    // Never hide native source unless the entire styled projection is exact.
    (i_slint_core::styled_text::get_raw_text(&styled) == source).then_some(styled)
}

pub(super) fn update_hook_highlight(page: &GitHubSettingsPage) {
    let g = page.global::<HookSettingsState>();
    let source = g.get_draft_script();
    let language = g.get_draft_language();
    let colors = g.get_code_colors();
    let styled = hook_styled_code(
        source.as_str(),
        language,
        &[
            colors.comment,
            colors.keyword,
            colors.literal,
            colors.constant,
            colors.entity,
            colors.variable,
        ],
    );
    g.set_code_highlight_ready(styled.is_some());
    g.set_code_highlight(styled.unwrap_or_default());
}

fn clear_hook_highlight(page: &GitHubSettingsPage) {
    let g = page.global::<HookSettingsState>();
    g.set_code_highlight_ready(false);
    g.set_code_highlight(slint::StyledText::default());
}

fn schedule_hook_highlight(page: &GitHubSettingsPage, timer: &Timer) {
    // Restart one reusable timer instead of queueing one single-shot timer for
    // every keystroke. A large pasted script therefore produces one bounded
    // projection update after editing settles, keeping the event loop responsive.
    timer.stop();
    let delayed = page.as_weak();
    timer.start(TimerMode::SingleShot, HOOK_HIGHLIGHT_DELAY, move || {
        if let Some(page) = delayed.upgrade() {
            if !page
                .global::<HookSettingsState>()
                .get_dialog_mode()
                .is_empty()
            {
                update_hook_highlight(&page);
            }
        }
    });
}

pub(crate) fn setup(
    ui: &MainWindow,
    settings: &crate::settings_page::SettingsPageBridge,
    center: MessageCenter,
) -> anyhow::Result<()> {
    let config = crate::app_paths::config_dir()
        .ok_or_else(|| anyhow::anyhow!("No Hook configuration directory"))?
        .join("hooks.json");
    let directory = crate::app_paths::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("No Hook data directory"))?
        .join("hooks/runs");
    let service = HookService::new(config, directory)?;
    let _ = SERVICE.set(service.clone());
    let notification_window = ui.as_weak();
    *service.observer.lock().unwrap() = Some(Arc::new(move |event, run| {
        if event.kind != "execution.started" && event.kind != "hook.finished" {
            return;
        }
        let started = event.kind == "execution.started";
        let severity = if started {
            MessageSeverity::Info
        } else if run.conclusion == Some(HookConclusion::Success) {
            MessageSeverity::Success
        } else {
            MessageSeverity::Warning
        };
        match center.publish(
            presentation::notification_draft(run, started)
                .severity(severity)
                .native_delivery(NativeDelivery::WhenUnfocused),
        ) {
            Ok(message) => {
                let window = notification_window.clone();
                let center = center.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = window.upgrade() {
                        crate::notification::show_recorded_message(&ui, &center, &message);
                    }
                });
            }
            Err(error) => log::warn!("[Hooks] Notification failed: {error}"),
        }
    }));
    let settings_for_delivery = settings.clone();
    let revisions = Mutex::new(None);
    let requested = Mutex::new(None);
    let cache = Arc::new(Mutex::new(ProjectionCache::default()));
    let preparation =
        crate::app_runtime::latest_compute::LatestCompute::<HookRun, RunProjection>::new();
    let deliver = crate::app_runtime::latest_ui(ui.as_weak(), move |ui, state: Arc<HookState>| {
        let next = (state.catalog_revision, state.durable_revision);
        let previous = revisions.lock().unwrap().replace(next);
        if previous != Some(next) {
            settings_for_delivery.apply_hooks(&state);
        }
        if previous.is_none_or(|(catalog, _)| catalog != state.catalog_revision) {
            crate::project::refresh_hook_choices(ui);
        }
        if ui.get_current_route() != Route::JobRun || ui.get_job_run_source() != "hooks" {
            preparation.cancel();
            requested.lock().unwrap().take();
            return;
        }
        let selected = SELECTED_RUN.lock().unwrap().clone();
        let Some(run) = state.runs.iter().find(|run| run.id == selected) else {
            return;
        };
        let key = (run.id.clone(), run.seq);
        if requested.lock().unwrap().replace(key.clone()) == Some(key) {
            return;
        }
        if LAST_PROJECTION
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(id, seq, _)| id == &run.id && *seq >= run.seq)
        {
            return;
        }
        let cache = cache.clone();
        let worker = preparation.clone();
        let weak = ui.as_weak();
        preparation.submit(
            run.clone(),
            move |run, token| {
                if token.cancelled() {
                    return None;
                }
                let projection = cache.lock().unwrap().prepare(&run);
                (!token.cancelled()).then_some(projection)
            },
            move || {
                let worker = worker.clone();
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    worker.acknowledge();
                    let Some(projection) = worker.take_output() else {
                        return;
                    };
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    if ui.get_current_route() == Route::JobRun
                        && ui.get_job_run_source() == "hooks"
                        && *SELECTED_RUN.lock().unwrap() == projection.id
                    {
                        apply_run(&ui, projection);
                    }
                });
            },
        );
    });
    let subscription = service.state.subscribe(Arc::new(move |snapshot| {
        deliver(snapshot.state_arc());
    }));
    let stop_service = service.clone();
    let log_service = service.clone();
    let log_ui = ui.as_weak();
    jobs::register_job_run_source(
        "hooks",
        move || stop_service.cancel(&SELECTED_RUN.lock().unwrap()),
        move |_| {
            let id = SELECTED_RUN.lock().unwrap().clone();
            load_full_log(log_service.clone(), log_ui.clone(), id);
        },
    );
    let loading = service.clone();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn_blocking(move || {
            // Hydration and orphan pruning finish before any run can be admitted.
            let _admission = loading.admission.blocking_lock();
            if loading.closed.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            let _ = loading.register_event(EventDefinition { name: PR_REVIEW_APPROVED.into(), category: CATEGORY_PR_REVIEW.into(), project_scoped: true, sample: serde_json::json!({"number":"123", "provider":"github", "repository":"https://github.com/example/repo", "action":"approved", "title":"Example pull request", "source_branch":"feature/example", "target_branch":"main", "author":"example-user", "author_name":"Example User", "author_email":"user@example.com"}) });
            let _ = loading.register_event(EventDefinition { name: PR_MERGED.into(), category: CATEGORY_PR_REVIEW.into(), project_scoped: true, sample: serde_json::json!({"number":"123", "provider":"github", "repository":"https://github.com/example/repo", "action":"merged", "merge_method":"squash", "remove_source_branch":true, "title":"Example pull request", "source_branch":"feature/example", "target_branch":"main", "author":"example-user", "author_name":"Example User", "author_email":"user@example.com"}) });
            #[cfg(feature = "ai-code-review")]
            let _ = loading.register_event(EventDefinition { name: AI_REVIEW_COMPLETED.into(), category: CATEGORY_AI_REVIEW.into(), project_scoped: true, sample: serde_json::json!({"number":"123", "task_id":"sample-task", "finding_count":2, "head_sha":"sample-head-sha", "title":"Example pull request", "source_branch":"feature/example", "target_branch":"main", "author":"example-user", "author_name":"Example User", "author_email":"user@example.com"}) });
            if let Err(error) = loading.state.hydrate() { log::error!("[Hooks] {error}"); }
            prune_logs(&loading);
        });
    });
    crate::shutdown::register_async_drain("hooks", move || {
        Box::pin(async move {
            cancel_replay();
            service.shutdown().await;
            UI_SUBSCRIPTION.lock().unwrap().take();
        })
    });
    *UI_SUBSCRIPTION.lock().unwrap() = Some(subscription);
    super::benchmark::setup(ui, current().unwrap());
    Ok(())
}

pub(super) fn benchmark_replay_ready(id: &str) -> bool {
    LAST_PROJECTION
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|(current, _, full)| current == id && *full)
}

fn prune_logs(service: &HookService) {
    let snapshot = service.state.snapshot();
    if !snapshot.state().ready {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(&service.directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "jsonl")
                && path
                    .file_stem()
                    .is_some_and(|id| !snapshot.state().runs.iter().any(|r| id == r.id.as_str()))
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

pub(crate) fn apply_settings(page: &GitHubSettingsPage, state: &HookState) {
    let global = page.global::<HookSettingsState>();
    global.set_ready(state.ready);
    global.set_enabled(state.enabled);
    global.set_can_clear_history(
        state.ready && state.runs.iter().any(|run| run.conclusion.is_some()),
    );
    global.set_summaries(sync_choices(
        global.get_summaries(),
        hook_summaries(state, global.get_query().as_str()),
    ));
    global.set_hooks(sync_choices(
        global.get_hooks(),
        state
            .hooks
            .iter()
            .map(|h| HookChoice {
                value: h.id.clone().into(),
                label: h.name.clone().into(),
            })
            .collect(),
    ));
    global.set_events(sync_choices(global.get_events(), event_choices(state)));
    let draft_event = global.get_draft_event();
    if !draft_event.is_empty() {
        global.set_draft_event_label(event_label(draft_event.as_str()).into());
        global.set_draft_event_index(event_choice_index(
            &global.get_events(),
            draft_event.as_str(),
        ));
    }
    global.set_history(sync_choices(
        global.get_history(),
        state
            .runs
            .iter()
            .rev()
            .map(|r| HookChoice {
                value: r.id.clone().into(),
                label: format!(
                    "{} · {} · {}",
                    r.name,
                    i18n::tr(r.status_key()),
                    DateTime::from_timestamp_millis(r.created_at_ms)
                        .map(|t| t.with_timezone(&Local).format("%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default()
                )
                .into(),
            })
            .collect(),
    ));
    if !state.error.is_empty() {
        global.set_result_error(true);
        set_feedback(
            page,
            i18n::LocalizedText::message("hooks.error.configuration"),
        );
    }
    let test_id = global.get_test_run();
    if let Some(run) = state.runs.iter().find(|r| r.id == test_id.as_str()) {
        global.set_busy(run.conclusion.is_none());
        global.set_result_error(run.conclusion.is_some_and(|c| c != HookConclusion::Success));
        global.set_test_status(
            match run.conclusion {
                None => "running",
                Some(HookConclusion::Success) => "success",
                Some(HookConclusion::Cancelled) => "cancelled",
                _ => "failure",
            }
            .into(),
        );
        global.set_result(
            if run.conclusion.is_some_and(|c| c != HookConclusion::Success) {
                run.detail_text().resolve().into()
            } else {
                "".into()
            },
        );
    }
}

pub(crate) fn sync_language(page: &GitHubSettingsPage) {
    if let Some(service) = current() {
        apply_settings(page, service.state.snapshot().state());
    }
    FEEDBACK.with(|slot| {
        if let Some((owner, text)) = slot.borrow().as_ref()
            && owner
                .upgrade()
                .is_some_and(|owner| std::ptr::eq(owner.window(), page.window()))
            && page.global::<HookSettingsState>().get_test_run().is_empty()
        {
            page.global::<HookSettingsState>()
                .set_result(text.resolve().into());
        }
    });
}

pub(crate) fn sync_job_language(ui: &MainWindow) {
    if ui.get_job_run_source() != "hooks" {
        return;
    }
    let selected = SELECTED_RUN.lock().unwrap().clone();
    let Some(service) = current() else { return };
    let snapshot = service.state.snapshot();
    let Some(run) = snapshot.state().runs.iter().find(|r| r.id == selected) else {
        return;
    };
    refresh_job_copy(ui, run);
}

pub(super) fn refresh_job_copy(ui: &MainWindow, run: &HookRun) {
    ui.set_job_run_summary(run.detail_text().resolve().into());
    ui.set_job_run_trigger_detail(
        presentation::trigger_detail(&run.event, &run.id, run.test).into(),
    );
    let rows = ui.get_job_run_steps();
    let Some(model) = rows.as_any().downcast_ref::<VecModel<crate::JobStepItem>>() else {
        return;
    };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else {
            continue;
        };
        row.detail = run.detail_text().resolve().into();
        model.set_row_data(index, row);
    }
}

fn event_label(event: &str) -> String {
    presentation::event_label_for(usher_i18n::active_locale().id, event)
}

fn event_category<'a>(state: &'a HookState, event_name: &str) -> &'a str {
    state
        .events
        .iter()
        .find(|event| event.name == event_name)
        .map(|event| event.category.as_str())
        .unwrap_or_else(|| match event_name {
            PR_REVIEW_APPROVED | PR_MERGED | LEGACY_PR_REVIEW_SUBMITTED => CATEGORY_PR_REVIEW,
            #[cfg(feature = "ai-code-review")]
            AI_REVIEW_COMPLETED => CATEGORY_AI_REVIEW,
            _ => CATEGORY_OTHER,
        })
}

fn category_label(category: &str) -> String {
    match category {
        CATEGORY_PR_REVIEW => i18n::tr("hooks.category_pr_review").into_owned(),
        CATEGORY_AI_REVIEW => i18n::tr("hooks.category_ai_review").into_owned(),
        CATEGORY_OTHER => i18n::tr("hooks.category_other").into_owned(),
        category => category.to_owned(),
    }
}

fn category_order(state: &HookState) -> Vec<String> {
    let mut categories = Vec::new();
    for event in state.events.iter() {
        if state.hooks.iter().any(|hook| hook.event == event.name)
            && !categories.contains(&event.category)
        {
            categories.push(event.category.clone());
        }
    }
    for hook in state.hooks.iter() {
        let category = event_category(state, &hook.event);
        if !categories.iter().any(|known| known == category) {
            categories.push(category.to_owned());
        }
    }
    categories
}

pub(super) fn hook_summaries(state: &HookState, query: &str) -> Vec<HookSummary> {
    let query = query.trim().to_lowercase();
    let mut summaries = Vec::with_capacity(state.hooks.len());
    let mut category_index = 0;
    for category in category_order(state) {
        let category_label = category_label(&category);
        let hooks = state
            .hooks
            .iter()
            .filter(|hook| event_category(state, &hook.event) == category)
            .filter(|hook| {
                query.is_empty()
                    || [
                        hook.name.as_str(),
                        hook.event.as_str(),
                        &event_label(&hook.event),
                        &category_label,
                        hook.language.label(),
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        if hooks.is_empty() {
            continue;
        }
        let category_size = hooks.len() as i32;
        for (index, hook) in hooks.into_iter().enumerate() {
            summaries.push(HookSummary {
                id: hook.id.clone().into(),
                name: hook.name.clone().into(),
                enabled: hook.enabled,
                description: format!("{} · {}", event_label(&hook.event), hook.language.label())
                    .into(),
                category: category_label.clone().into(),
                category_index,
                category_start: index == 0,
                category_end: index as i32 == category_size - 1,
                category_size,
            });
        }
        category_index += 1;
    }
    summaries
}

fn event_choices(state: &HookState) -> Vec<HookEventChoice> {
    let mut categories = Vec::new();
    for event in state.events.iter() {
        if !categories.contains(&event.category) {
            categories.push(event.category.clone());
        }
    }
    let mut choices = Vec::with_capacity(state.events.len());
    for (group_index, category) in categories.iter().enumerate() {
        let events = state
            .events
            .iter()
            .filter(|event| event.category == *category)
            .collect::<Vec<_>>();
        let group_size = events.len() as i32;
        let group_label = category_label(category);
        for (index, event) in events.into_iter().enumerate() {
            choices.push(HookEventChoice {
                value: event.name.clone().into(),
                label: event_label(&event.name).into(),
                group_label: group_label.clone().into(),
                group_index: group_index as i32,
                group_start: index == 0,
                group_size,
            });
        }
    }
    choices
}

fn event_choice_index(events: &ModelRc<HookEventChoice>, event: &str) -> i32 {
    (0..events.row_count())
        .find(|index| {
            events
                .row_data(*index)
                .is_some_and(|choice| choice.value == event)
        })
        .map_or(0, |index| index as i32)
}

fn sync_choices<T: Clone + PartialEq + 'static>(current: ModelRc<T>, next: Vec<T>) -> ModelRc<T> {
    if let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() {
        while model.row_count() > next.len() {
            model.remove(model.row_count() - 1);
        }
        for (index, item) in next.into_iter().enumerate() {
            if index == model.row_count() {
                model.push(item);
            } else if model.row_data(index).as_ref() != Some(&item) {
                model.set_row_data(index, item);
            }
        }
        current
    } else {
        ModelRc::new(VecModel::from(next))
    }
}

fn set_draft_metadata(page: &GitHubSettingsPage, hook: &HookDefinition) {
    FEEDBACK.with(|slot| slot.borrow_mut().take());
    let g = page.global::<HookSettingsState>();
    g.set_draft_id(hook.id.clone().into());
    g.set_draft_name(hook.name.clone().into());
    g.set_draft_enabled(hook.enabled);
    g.set_draft_event(hook.event.clone().into());
    g.set_draft_event_label(event_label(&hook.event).into());
    g.set_draft_event_index(event_choice_index(&g.get_events(), &hook.event));
    g.set_draft_project(hook.project.clone().into());
    g.set_draft_outcome(hook.outcome.clone().into());
    g.set_draft_language(match hook.language {
        HookLanguage::Shell => 0,
        HookLanguage::JavaScriptTypeScript => 1,
    });
    g.set_draft_interpreter(hook.interpreter.clone().into());
    g.set_draft_script_loading(false);
    g.set_draft_timeout(hook.timeout_seconds.to_string().into());
    g.set_shell_draft(HookLanguageDraft::default());
    g.set_node_draft(HookLanguageDraft::default());
    g.set_result("".into());
    g.set_result_error(false);
    g.set_test_run("".into());
    g.set_test_status("".into());
}

pub(super) fn set_draft(page: &GitHubSettingsPage, hook: &HookDefinition) {
    set_draft_metadata(page, hook);
    page.global::<HookSettingsState>()
        .set_draft_script(hook.script.clone().into());
}

fn open_editor_draft(
    page: &GitHubSettingsPage,
    hook: &HookDefinition,
    script_timer: &Timer,
    highlight_timer: Rc<Timer>,
) {
    script_timer.stop();
    highlight_timer.stop();
    set_draft_metadata(page, hook);

    let g = page.global::<HookSettingsState>();
    g.set_draft_script("".into());
    clear_hook_highlight(page);
    g.set_draft_script_loading(!hook.script.is_empty());
    g.set_dialog_mode("edit".into());
    if hook.script.is_empty() {
        return;
    }

    let page = page.as_weak();
    let expected_id = hook.id.clone();
    let script = hook.script.clone();
    script_timer.start(
        TimerMode::SingleShot,
        HOOK_SCRIPT_HYDRATION_DELAY,
        move || {
            let Some(page) = page.upgrade() else {
                return;
            };
            let g = page.global::<HookSettingsState>();
            if g.get_dialog_mode() != "edit" || g.get_draft_id() != expected_id {
                return;
            }
            g.set_draft_script(script.clone().into());
            g.set_draft_script_loading(false);
            schedule_hook_highlight(&page, &highlight_timer);
        },
    );
}

fn draft(page: &GitHubSettingsPage) -> Result<HookDefinition, HookValidationError> {
    let g = page.global::<HookSettingsState>();
    let hook = HookDefinition {
        id: g.get_draft_id().into(),
        revision: 0,
        name: g.get_draft_name().into(),
        enabled: g.get_draft_enabled(),
        event: g.get_draft_event().into(),
        project: g.get_draft_project().trim().into(),
        outcome: g.get_draft_outcome().trim().into(),
        language: match g.get_draft_language() {
            1 => HookLanguage::JavaScriptTypeScript,
            _ => HookLanguage::Shell,
        },
        interpreter: g.get_draft_interpreter().trim().into(),
        script: g.get_draft_script().into(),
        timeout_seconds: g
            .get_draft_timeout()
            .trim()
            .parse()
            .map_err(|_| HookValidationError::TimeoutInteger)?,
    };
    hook.validate()?;
    Ok(hook)
}

thread_local! {
    static FEEDBACK: std::cell::RefCell<Option<(slint::Weak<GitHubSettingsPage>, i18n::LocalizedText)>> = const { std::cell::RefCell::new(None) };
}

fn set_feedback(page: &GitHubSettingsPage, text: i18n::LocalizedText) {
    page.global::<HookSettingsState>()
        .set_result(text.resolve().into());
    FEEDBACK.with(|slot| *slot.borrow_mut() = Some((page.as_weak(), text)));
}

trait SettingsError {
    fn feedback(&self) -> i18n::LocalizedText;
}
impl SettingsError for HookValidationError {
    fn feedback(&self) -> i18n::LocalizedText {
        self.localized()
    }
}
impl SettingsError for String {
    fn feedback(&self) -> i18n::LocalizedText {
        log::warn!("[Hooks] Settings operation failed: {self}");
        i18n::LocalizedText::message("hooks.error.command")
    }
}

fn show_error(page: &GitHubSettingsPage, error: impl SettingsError) {
    let g = page.global::<HookSettingsState>();
    g.set_test_run("".into());
    if g.get_test_status() == "running" {
        g.set_test_status("failure".into());
    }
    g.set_result_error(true);
    set_feedback(page, error.feedback());
}

pub(crate) fn bind_settings(ui: &MainWindow, page: &GitHubSettingsPage) {
    let Some(service) = current() else {
        return;
    };
    bind_settings_with_service(ui, page, service);
}

pub(super) fn bind_settings_with_service(
    ui: &MainWindow,
    page: &GitHubSettingsPage,
    service: Arc<HookService>,
) {
    bind_language_selector(page);
    bind_history_actions(page, service.clone());
    apply_settings(page, service.state.snapshot().state());
    let weak = page.as_weak();
    let search_service = service.clone();
    page.global::<HookSettingsState>()
        .on_query_hooks(move |query| {
            if let Some(page) = weak.upgrade() {
                page.global::<HookSettingsState>().set_query(query);
                apply_settings(&page, search_service.state.snapshot().state());
            }
        });
    page.global::<HookSettingsState>()
        .set_code_font_family(ui.get_workflow_pr_review_code_font_family());
    page.global::<HookSettingsState>()
        .set_native_cursor_uses_selection_color(cfg!(target_os = "macos"));
    let highlight_timer = Rc::new(Timer::default());
    let script_timer = Rc::new(Timer::default());
    let weak = page.as_weak();
    let highlight_timer_for_theme = highlight_timer.clone();
    page.global::<HookSettingsState>()
        .on_refresh_highlight(move || {
            if let Some(page) = weak.upgrade() {
                schedule_hook_highlight(&page, &highlight_timer_for_theme);
            }
        });
    let weak = page.as_weak();
    let highlight_timer_for_edit = highlight_timer.clone();
    page.global::<HookSettingsState>().on_draft_edited(move || {
        if let Some(page) = weak.upgrade() {
            clear_hook_highlight(&page);
            schedule_hook_highlight(&page, &highlight_timer_for_edit);
            if !page.global::<HookSettingsState>().get_busy() {
                reset_test_feedback(&page);
            }
        }
    });
    let weak = page.as_weak();
    let highlight_timer_for_create = highlight_timer.clone();
    let script_timer_for_create = script_timer.clone();
    page.global::<HookSettingsState>().on_create(move || {
        if let Some(page) = weak.upgrade() {
            if page.global::<HookSettingsState>().get_busy() {
                return;
            }
            script_timer_for_create.stop();
            highlight_timer_for_create.stop();
            set_draft(&page, &HookDefinition::default());
            page.global::<HookSettingsState>()
                .set_dialog_mode("add".into());
            schedule_hook_highlight(&page, &highlight_timer_for_create);
        }
    });
    let weak = page.as_weak();
    let highlight_timer_for_cancel = highlight_timer.clone();
    let script_timer_for_cancel = script_timer.clone();
    page.global::<HookSettingsState>().on_cancel(move || {
        if let Some(page) = weak.upgrade() {
            if !page.global::<HookSettingsState>().get_busy() {
                script_timer_for_cancel.stop();
                highlight_timer_for_cancel.stop();
                close_editor(&page);
            }
        }
    });
    let weak = page.as_weak();
    let deleting = service.clone();
    let highlight_timer_for_delete = highlight_timer.clone();
    let script_timer_for_delete = script_timer.clone();
    page.global::<HookSettingsState>()
        .on_request_delete(move |id| {
            if let Some(page) = weak.upgrade() {
                if page.global::<HookSettingsState>().get_busy() {
                    return;
                }
                if let Some(hook) = deleting
                    .state
                    .snapshot()
                    .state()
                    .hooks
                    .iter()
                    .find(|hook| hook.id == id.as_str())
                {
                    script_timer_for_delete.stop();
                    highlight_timer_for_delete.stop();
                    set_draft_metadata(&page, hook);
                    let g = page.global::<HookSettingsState>();
                    g.set_draft_script("".into());
                    clear_hook_highlight(&page);
                    g.set_dialog_mode("delete".into());
                }
            }
        });
    let weak = page.as_weak();
    let ui_weak = ui.as_weak();
    let select_service = service.clone();
    let highlight_timer_for_choose = highlight_timer.clone();
    let script_timer_for_choose = script_timer.clone();
    page.global::<HookSettingsState>()
        .on_choose(move |kind, value| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.global::<HookSettingsState>().get_busy() {
                return;
            }
            match kind.as_str() {
                "event" => {
                    reset_test_feedback(&page);
                    let global = page.global::<HookSettingsState>();
                    global.set_draft_event_label(event_label(value.as_str()).into());
                    global.set_draft_event_index(event_choice_index(&global.get_events(), &value));
                    global.set_draft_event(value);
                }
                "hook" => {
                    if let Some(hook) = select_service
                        .state
                        .snapshot()
                        .state()
                        .hooks
                        .iter()
                        .find(|h| h.id == value.as_str())
                    {
                        open_editor_draft(
                            &page,
                            hook,
                            &script_timer_for_choose,
                            highlight_timer_for_choose.clone(),
                        );
                    }
                }
                "history" => {
                    if let Some(ui) = ui_weak.upgrade() {
                        open_run(&ui, value.as_str());
                    }
                }
                _ => {}
            }
        });
    let weak = page.as_weak();
    let ui_weak = ui.as_weak();
    let test_service = service.clone();
    page.global::<HookSettingsState>().on_test(move || {
        let Some(page) = weak.upgrade() else {
            return;
        };
        if page.global::<HookSettingsState>().get_busy() {
            return;
        }
        reset_test_feedback(&page);
        page.global::<HookSettingsState>()
            .set_test_status("running".into());
        let hook = match draft(&page) {
            Ok(h) => h,
            Err(e) => {
                show_error(&page, e);
                return;
            }
        };
        page.global::<HookSettingsState>().set_busy(true);
        page.global::<HookSettingsState>().set_test_run("".into());
        let page_weak = page.as_weak();
        let window = ui_weak.clone();
        let service = test_service.clone();
        crate::app_runtime::shared_runtime().spawn(async move {
            let result = service.test(hook, None).await;
            let wait_id = result.as_ref().ok().cloned();
            let wait_service = service.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if window.upgrade().is_none() {
                    return;
                }
                if let Some(page) = page_weak.upgrade() {
                    match result {
                        Ok(id) => {
                            page.global::<HookSettingsState>().set_test_run(id.into());
                            apply_settings(&page, service.state.snapshot().state());
                        }
                        Err(error) => {
                            page.global::<HookSettingsState>().set_busy(false);
                            show_error(&page, error);
                        }
                    }
                }
            });
            if let Some(id) = wait_id {
                let _ = wait_service.wait(&id).await;
            }
        });
    });
    let weak = page.as_weak();
    let save_service = service.clone();
    page.global::<HookSettingsState>().on_save(move || {
        let Some(page) = weak.upgrade() else {
            return;
        };
        if page.global::<HookSettingsState>().get_busy() {
            return;
        }
        reset_test_feedback(&page);
        match draft(&page) {
            Ok(hook) => {
                let service = save_service.clone();
                settings_command(&page, move || service.save(hook), "hooks.saved", true);
            }
            Err(error) => show_error(&page, error),
        }
    });
    let weak = page.as_weak();
    let delete_service = service.clone();
    page.global::<HookSettingsState>().on_delete(move || {
        if let Some(page) = weak.upgrade() {
            let id = page
                .global::<HookSettingsState>()
                .get_draft_id()
                .to_string();
            let service = delete_service.clone();
            settings_command(&page, move || service.delete(id), "hooks.deleted", true);
        }
    });
    let weak = page.as_weak();
    let enabled_service = service.clone();
    page.global::<HookSettingsState>()
        .on_enable(move |enabled| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let g = page.global::<HookSettingsState>();
            if g.get_busy() || g.get_enable_busy() {
                return;
            }
            g.set_enable_busy(true);
            let weak = page.as_weak();
            let service = enabled_service.clone();
            crate::app_runtime::shared_runtime().spawn_blocking(move || {
                let result = service.enable(enabled);
                let snapshot = service.state.snapshot();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(page) = weak.upgrade() {
                        let g = page.global::<HookSettingsState>();
                        g.set_enable_busy(false);
                        apply_settings(&page, snapshot.state());
                        match result {
                            Ok(()) => {
                                g.set_result_error(false);
                                set_feedback(&page, i18n::LocalizedText::message("hooks.saved"));
                            }
                            Err(error) => show_error(&page, error),
                        }
                    }
                });
            });
        });
    let weak = page.as_weak();
    let row_enabled_service = service.clone();
    page.global::<HookSettingsState>()
        .on_set_hook_enabled(move |id, enabled| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let g = page.global::<HookSettingsState>();
            if g.get_busy() || !g.get_enabled() || !g.get_row_enable_busy_id().is_empty() {
                return;
            }
            g.set_row_enable_busy_id(id.clone());
            let id = id.to_string();
            let weak = page.as_weak();
            let service = row_enabled_service.clone();
            crate::app_runtime::shared_runtime().spawn_blocking(move || {
                let result = service.set_hook_enabled(id.clone(), enabled);
                let snapshot = service.state.snapshot();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(page) = weak.upgrade() {
                        let g = page.global::<HookSettingsState>();
                        if g.get_row_enable_busy_id().as_str() == id {
                            g.set_row_enable_busy_id("".into());
                        }
                        apply_settings(&page, snapshot.state());
                        match result {
                            Ok(()) => {
                                g.set_result_error(false);
                                set_feedback(&page, i18n::LocalizedText::message("hooks.saved"));
                            }
                            Err(error) => {
                                show_error(&page, error);
                            }
                        }
                    }
                });
            });
        });
    let weak = page.as_weak();
    page.global::<HookSettingsState>().on_stop(move || {
        if let Some(page) = weak.upgrade() {
            service.cancel(page.global::<HookSettingsState>().get_test_run().as_str());
        }
    });
    let ui_weak = ui.as_weak();
    let weak = page.as_weak();
    page.global::<HookSettingsState>().on_open_run(move |id| {
        if let Some(ui) = ui_weak.upgrade() {
            if let Some(page) = weak.upgrade() {
                if page.global::<HookSettingsState>().get_busy() {
                    return;
                }
                close_editor(&page);
            }
            open_run(&ui, id.as_str());
        }
    });
}

fn reset_test_feedback(page: &GitHubSettingsPage) {
    FEEDBACK.with(|slot| slot.borrow_mut().take());
    let g = page.global::<HookSettingsState>();
    g.set_test_run("".into());
    g.set_test_status("".into());
    g.set_result("".into());
    g.set_result_error(false);
}

pub(crate) fn handle_dialog_key(
    page: &GitHubSettingsPage,
    text: &str,
    primary: bool,
    alt: bool,
    shift: bool,
    secondary: bool,
) -> bool {
    let g = page.global::<HookSettingsState>();
    if g.get_dialog_mode().is_empty() || alt || shift || secondary {
        return false;
    }
    if !primary && text == slint::SharedString::from(slint::platform::Key::Escape).as_str() {
        if page.invoke_dismiss_hook_menu() {
            return true;
        }
        if !g.get_busy() {
            g.invoke_cancel();
        }
        return true;
    }
    if primary
        && text == slint::SharedString::from(slint::platform::Key::Return).as_str()
        && matches!(g.get_dialog_mode().as_str(), "add" | "edit")
    {
        if g.get_ready() && !g.get_busy() {
            g.invoke_save();
        }
        return true;
    }
    false
}

fn bind_history_actions(page: &GitHubSettingsPage, service: Arc<HookService>) {
    let weak = page.as_weak();
    page.global::<HookSettingsState>()
        .on_request_clear_history(move || {
            if let Some(page) = weak.upgrade() {
                let g = page.global::<HookSettingsState>();
                if !g.get_busy() && g.get_can_clear_history() {
                    reset_test_feedback(&page);
                    g.set_dialog_mode("clear-history".into());
                }
            }
        });
    let weak = page.as_weak();
    page.global::<HookSettingsState>()
        .on_clear_history(move || {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let g = page.global::<HookSettingsState>();
            if g.get_busy() || g.get_dialog_mode() != "clear-history" {
                return;
            }
            g.set_busy(true);
            reset_test_feedback(&page);
            let weak = page.as_weak();
            let service = service.clone();
            crate::app_runtime::shared_runtime().spawn(async move {
                let result = service.clear_history().await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(page) = weak.upgrade() {
                        page.global::<HookSettingsState>().set_busy(false);
                        apply_settings(&page, service.state.snapshot().state());
                        match result {
                            Ok(()) => close_editor(&page),
                            Err(error) => show_error(&page, error),
                        }
                    }
                });
            });
        });
}

pub(super) fn bind_language_selector(page: &GitHubSettingsPage) {
    let highlight_timer = Rc::new(Timer::default());
    let weak = page.as_weak();
    let highlight_timer_for_language = highlight_timer.clone();
    page.global::<HookSettingsState>()
        .on_select_language(move |language| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let g = page.global::<HookSettingsState>();
            if !g.get_ready()
                || g.get_busy()
                || !(0..=1).contains(&language)
                || language == g.get_draft_language()
            {
                return;
            }
            let previous = HookLanguageDraft {
                script: g.get_draft_script(),
                interpreter: g.get_draft_interpreter(),
            };
            match g.get_draft_language() {
                0 => g.set_shell_draft(previous),
                1 => g.set_node_draft(previous),
                _ => {}
            }
            let selected = match language {
                1 => g.get_node_draft(),
                _ => g.get_shell_draft(),
            };
            clear_hook_highlight(&page);
            g.set_draft_language(language);
            g.set_draft_script(selected.script);
            g.set_draft_interpreter(selected.interpreter);
            // The language switch changes tokenization even when the script
            // text stays the same. The draft-edited callback is not emitted by
            // these programmatic bindings, so refresh the bounded projection
            // explicitly on the next debounce tick.
            schedule_hook_highlight(&page, &highlight_timer_for_language);
            reset_test_feedback(&page);
        });
}

fn close_editor(page: &GitHubSettingsPage) {
    page.invoke_close_hook_editor();
    set_draft(page, &HookDefinition::default());
    clear_hook_highlight(page);
}

fn settings_command(
    page: &GitHubSettingsPage,
    command: impl FnOnce() -> Result<(), String> + Send + 'static,
    success: &'static str,
    close_on_success: bool,
) {
    let g = page.global::<HookSettingsState>();
    if g.get_busy() {
        return;
    }
    g.set_busy(true);
    reset_test_feedback(page);
    let weak = page.as_weak();
    crate::app_runtime::shared_runtime().spawn_blocking(move || {
        let result = command();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(page) = weak.upgrade() {
                let g = page.global::<HookSettingsState>();
                g.set_busy(false);
                match result {
                    Ok(()) => {
                        if close_on_success {
                            if g.get_dialog_mode() == "add" {
                                g.invoke_query_hooks("".into());
                            }
                            close_editor(&page);
                        }
                        g.set_result_error(false);
                        set_feedback(&page, i18n::LocalizedText::message(success));
                    }
                    Err(error) => show_error(&page, error),
                }
            }
        });
    });
}

fn apply_run(ui: &MainWindow, mut projection: RunProjection) {
    {
        let mut last = LAST_PROJECTION.lock().unwrap();
        if last.as_ref().is_some_and(|(id, seq, full)| {
            id == &projection.id
                && (*seq > projection.seq
                    || (*seq == projection.seq && (*full || !projection.full_log)))
        }) {
            return;
        }
        *last = Some((projection.id.clone(), projection.seq, projection.full_log));
    }
    if !projection.running && !projection.full_log {
        if let Some(service) = current() {
            load_full_log(service, ui.as_weak(), projection.id.clone());
        }
    }
    if projection.full_log && crate::app_paths::benchmark_offline_mode() {
        if let Ok(batch) = std::env::var("USHER_P6_BENCHMARK") {
            log::info!(
                "[P6][{batch}] replay_applied terminal={}",
                projection
                    .job
                    .steps
                    .iter()
                    .any(|s| s.log.contains("[hook.finished]"))
            );
        }
    }
    projection.descriptor.trigger_detail =
        presentation::trigger_detail(&projection.event, &projection.id, projection.test).into();
    jobs::apply_job_run(
        ui,
        &projection.descriptor,
        &projection.job,
        &projection.detail,
        projection.running,
    );
}

pub(crate) fn open_target(ui: &MainWindow, target: &str) -> bool {
    let Some(id) = target.strip_prefix("hook-run:") else {
        return false;
    };
    open_run(ui, id);
    true
}
fn open_run(ui: &MainWindow, id: &str) {
    let Some(service) = current() else {
        return;
    };
    if !service
        .state
        .snapshot()
        .state()
        .runs
        .iter()
        .any(|r| r.id == id)
    {
        crate::toast::localized_error(ui, "hooks.error.expired");
        return;
    }
    *SELECTED_RUN.lock().unwrap() = id.into();
    *LAST_PROJECTION.lock().unwrap() = None;
    ui.set_job_run_source("hooks".into());
    ui.invoke_navigate(Route::JobRun);
    load_full_log(service, ui.as_weak(), id.into());
}
fn load_full_log(service: Arc<HookService>, weak: slint::Weak<MainWindow>, id: String) {
    cancel_replay();
    let generation = REPLAY_GENERATION.load(Ordering::Acquire);
    let task = crate::app_runtime::shared_runtime().spawn(async move {
        // Live replay uses the captured sequence; recovered runs have no writer.
        let Some(mut run) = service
            .state
            .snapshot()
            .state()
            .runs
            .iter()
            .find(|r| r.id == id)
            .cloned()
        else {
            return;
        };
        let result = replay::read_log(
            &service.directory.join(format!("{id}.jsonl")),
            &mut run,
            || REPLAY_GENERATION.load(Ordering::Acquire) != generation,
        )
        .await;
        if REPLAY_GENERATION.load(Ordering::Acquire) != generation {
            return;
        }
        match result {
            Ok(text) => run.log = Arc::new(text),
            Err(error) => {
                let diagnostic: String = run.detail.chars().take(MAX_TAIL_BYTES / 4).collect();
                run.log = Arc::new(format!(
                    "[log.unavailable] {error}\n[summary.detail] {diagnostic}\n{}",
                    run.log
                ));
            }
        }
        let mut projection = project_run(&run);
        projection.full_log = true;
        let _ = slint::invoke_from_event_loop(move || {
            if REPLAY_GENERATION.load(Ordering::Acquire) == generation
                && *SELECTED_RUN.lock().unwrap() == id
            {
                if let Some(ui) = weak.upgrade() {
                    if ui.get_job_run_source() == "hooks" && ui.get_current_route() == Route::JobRun
                    {
                        apply_run(&ui, projection);
                    }
                }
            }
        });
    });
    *REPLAY_TASK.lock().unwrap() = Some(task.abort_handle());
}
