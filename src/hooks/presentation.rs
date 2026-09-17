//! Semantic Hook copy, resolved only at the presentation boundary.
use super::*;
use crate::i18n::LocalizedText;

impl HookConclusion {
    pub(super) fn message_key(self) -> &'static str {
        match self {
            Self::Success => "hooks.conclusion.success",
            Self::Failure => "hooks.conclusion.failure",
            Self::TimedOut => "hooks.conclusion.timed_out",
            Self::Cancelled => "hooks.status_cancelled",
            Self::StartFailed => "hooks.conclusion.start_failed",
            Self::Interrupted => "hooks.conclusion.interrupted",
            Self::Skipped => "hooks.conclusion.skipped",
        }
    }
}

impl HookFailureKind {
    pub(super) fn message_key(self) -> &'static str {
        match self {
            Self::Journal => "hooks.error.journal",
            Self::Preparation => "hooks.error.preparation",
            Self::WorkingDirectory => "hooks.error.working_directory",
            Self::InterpreterStart => "hooks.error.interpreter_start",
            Self::ProcessTree => "hooks.error.process_tree",
            Self::Wait => "hooks.error.wait",
            Self::OutputDrain => "hooks.error.output_drain",
            Self::InvalidDefinition => "hooks.error.invalid_definition",
            Self::Unknown => "hooks.error.execution",
        }
    }
}

impl HookValidationError {
    pub(super) fn localized(self) -> LocalizedText {
        LocalizedText::message(match self {
            Self::InvalidId => "hooks.validation.invalid_id",
            Self::NameRequired => "hooks.validation.name_required",
            Self::ScriptEmpty => "hooks.validation.script_empty",
            Self::ScriptTooLarge => "hooks.validation.script_too_large",
            Self::TimeoutRange => "hooks.validation.timeout_range",
            Self::FieldTooLong => "hooks.validation.field_too_long",
            Self::InterpreterPath => "hooks.validation.interpreter_path",
            Self::TimeoutInteger => "hooks.validation.timeout_integer",
        })
    }
}

impl HookRun {
    pub(super) fn status_key(&self) -> &'static str {
        self.conclusion.map(HookConclusion::message_key).unwrap_or(
            if self.started_at_ms.is_some() {
                "hooks.running"
            } else {
                "hooks.queued"
            },
        )
    }

    pub(super) fn detail_text(&self) -> LocalizedText {
        if let Some(kind) = self.failure_kind {
            return LocalizedText::message(kind.message_key());
        }
        if self.conclusion == Some(HookConclusion::Failure) {
            if let Some(signal) = self.signal {
                return LocalizedText::message_with(
                    "hooks.error.signal",
                    [("signal", signal.to_string())],
                );
            }
            if let Some(code) = self.exit_code {
                return LocalizedText::message_with(
                    "hooks.error.exit_code",
                    [("code", code.to_string())],
                );
            }
        }
        // Legacy details remain available in the journal, never guessed from arbitrary prose.
        LocalizedText::message(self.status_key())
    }
}

pub(super) fn event_label_for(locale: &str, event: &str) -> String {
    let key = match event {
        PR_REVIEW_APPROVED => "hooks.event_pr_approved",
        PR_MERGED => "hooks.event_pr_merged",
        "ai.review.completed.v1" => "hooks.event_ai_review",
        _ => return event.to_owned(),
    };
    usher_i18n::translate_for(locale, key).into_owned()
}

pub(super) fn trigger_detail(event: &str, id: &str, test: bool) -> String {
    let locale = usher_i18n::active_locale().id;
    let caption = event_label_for(locale, event);
    let caption = if test {
        usher_i18n::translate_for_with(locale, "hooks.test_prefix", &[("name", caption.as_str())])
    } else {
        caption
    };
    format!("{caption} · {id}")
}

pub(super) fn notification_draft(run: &HookRun, started: bool) -> usher_messages::MessageDraft {
    // Durable fallback fields contain only the user-owned name, never a rendered translation.
    usher_messages::MessageDraft::new(&run.name, "")
        .source("Hooks")
        .category("hooks")
        .target(format!("hook-run:{}", run.id))
        .metadata("hooks.copy", "1")
        .metadata("hooks.name", &run.name)
        .metadata("hooks.event", &run.event)
        .metadata("hooks.test", if run.test { "true" } else { "false" })
        .metadata("hooks.stage", if started { "started" } else { "finished" })
        .metadata(
            "hooks.conclusion",
            run.conclusion
                .map(HookConclusion::label)
                .unwrap_or("running"),
        )
}

pub(crate) fn notification_copy(
    message: &usher_messages::Message,
    locale: &str,
) -> Option<(String, String)> {
    if message.source != "Hooks"
        || message.channel != "application"
        || message.category != "hooks"
        || message.metadata.get("hooks.copy").map(String::as_str) != Some("1")
        || !message.target.as_deref()?.starts_with("hook-run:")
    {
        return None;
    }
    let fields = &message.metadata;
    let name = fields.get("hooks.name")?;
    let name = match fields.get("hooks.test")?.as_str() {
        "true" => {
            usher_i18n::translate_for_with(locale, "hooks.test_prefix", &[("name", name.as_str())])
        }
        "false" => name.clone(),
        _ => return None,
    };
    let (title_key, status_key) = match fields.get("hooks.stage")?.as_str() {
        "started" => ("hooks.started", "hooks.running"),
        "finished" => {
            let conclusion: HookConclusion = serde_json::from_value(serde_json::Value::String(
                fields.get("hooks.conclusion")?.clone(),
            ))
            .ok()?;
            ("hooks.finished", conclusion.message_key())
        }
        _ => return None,
    };
    let event = event_label_for(locale, fields.get("hooks.event")?);
    let status = usher_i18n::translate_for(locale, status_key);
    Some((
        usher_i18n::translate_for_with(locale, title_key, &[("name", name.as_str())]),
        usher_i18n::translate_for_with(
            locale,
            "hooks.notification_body",
            &[("event", event.as_str()), ("status", status.as_ref())],
        ),
    ))
}
