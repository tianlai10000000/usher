//! Source-controlled prompt catalog and typed AI Agent request assembly.

use std::sync::OnceLock;

use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};

use super::agent::CompletionMessage;
use crate::settings::AiAgentSettings;

#[cfg(feature = "ai-code-review")]
use usher_ai_review::{
    PromptContextEntry, PromptProvenance, ReviewConversationMessage, ReviewConversationRole,
    ReviewInput, ReviewResult,
};

const PROMPT_SOURCE: &str = include_str!("prompts.toml");
const MAX_CONVERSATION_MESSAGES: usize = 20;
#[cfg(feature = "ai-code-review")]
const MAX_REVIEW_CHAT_DIFF_BYTES: usize = 48 * 1024;

#[derive(Debug, Deserialize)]
struct PromptCatalog {
    schema_version: u32,
    shared: SharedPrompts,
    general_chat: PromptTemplate,
    review_chunk: PromptTemplate,
    review_conversation: PromptTemplate,
}

#[derive(Debug, Deserialize)]
struct SharedPrompts {
    reply_language: PromptFragment,
}

#[derive(Debug, Deserialize)]
struct PromptFragment {
    template: String,
}

#[derive(Debug, Deserialize)]
struct PromptTemplate {
    id: String,
    version: u32,
    system: String,
    #[serde(default)]
    user: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PromptEnvelope {
    pub id: String,
    pub version: u32,
    pub effective_reply_language: String,
    pub messages: Vec<CompletionMessage>,
    context: Vec<PromptContextMetadata>,
}

#[derive(Debug, Clone)]
struct PromptContextMetadata {
    #[cfg_attr(not(feature = "ai-code-review"), allow(dead_code))]
    name: String,
    bytes: u64,
    truncated: bool,
}

impl PromptEnvelope {
    pub(crate) fn log_metadata(&self, operation: &str) {
        let (context_fields, context_bytes, truncated_fields) = (
            self.context.len(),
            self.context.iter().map(|item| item.bytes).sum::<u64>(),
            self.context.iter().filter(|item| item.truncated).count(),
        );
        log::debug!(
            "[AI Prompt] operation={} prompt_id={} prompt_version={} reply_language={} messages={} context_fields={} context_bytes={} truncated_fields={}",
            operation,
            self.id,
            self.version,
            self.effective_reply_language,
            self.messages.len(),
            context_fields,
            context_bytes,
            truncated_fields,
        );
    }

    #[cfg(feature = "ai-code-review")]
    pub(crate) fn provenance(&self, request_count: u32) -> PromptProvenance {
        PromptProvenance {
            prompt_id: self.id.clone(),
            prompt_version: self.version,
            effective_reply_language: self.effective_reply_language.clone(),
            request_count,
            context: self
                .context
                .iter()
                .map(|entry| PromptContextEntry {
                    name: entry.name.clone(),
                    bytes: entry.bytes,
                    truncated: entry.truncated,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct LanguageContext<'a> {
    reply_language_name: &'a str,
    structured_output: bool,
}

#[derive(Serialize)]
struct SystemContext<'a> {
    reply_language_instruction: &'a str,
}

#[cfg(feature = "ai-code-review")]
#[derive(Serialize)]
struct ReviewChunkContext<'a> {
    project_name: &'a str,
    pull_request_number: &'a str,
    pull_request_title: &'a str,
    source_branch: &'a str,
    target_branch: &'a str,
    base_sha: &'a str,
    head_sha: &'a str,
    review_rules: &'a str,
    additional_requirements: &'a str,
    chunk_index: usize,
    chunk_count: usize,
    diff_chunk: &'a str,
}

#[cfg(feature = "ai-code-review")]
#[derive(Serialize)]
struct ReviewConversationContext<'a> {
    project_name: &'a str,
    pull_request_number: &'a str,
    pull_request_title: &'a str,
    source_branch: &'a str,
    target_branch: &'a str,
    base_sha: &'a str,
    head_sha: &'a str,
    diff_excerpt: &'a str,
    completed_review: &'a str,
}

fn catalog() -> Result<&'static PromptCatalog, String> {
    static CATALOG: OnceLock<Result<PromptCatalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let parsed = toml::from_str::<PromptCatalog>(PROMPT_SOURCE)
                .map_err(|error| format!("invalid built-in AI prompt catalog: {error}"))?;
            if parsed.schema_version != 1 {
                return Err(format!(
                    "unsupported built-in AI prompt schema version {}",
                    parsed.schema_version
                ));
            }
            validate_template(&parsed.shared.reply_language.template)?;
            for template in [
                &parsed.general_chat,
                &parsed.review_chunk,
                &parsed.review_conversation,
            ] {
                if template.id.trim().is_empty() || template.version == 0 {
                    return Err("built-in AI prompt ids and versions must be non-zero".to_string());
                }
                validate_template(&template.system)?;
                if !template.user.is_empty() {
                    validate_template(&template.user)?;
                }
            }
            Ok(parsed)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn environment() -> Environment<'static> {
    let mut environment = Environment::empty();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment
}

fn validate_template(source: &str) -> Result<(), String> {
    environment()
        .template_from_str(source)
        .map(|_| ())
        .map_err(|error| format!("invalid built-in AI prompt template: {error}"))
}

fn render(source: &str, context: impl Serialize) -> Result<String, String> {
    environment()
        .render_str(source, context)
        .map(|rendered| rendered.trim().to_string())
        .map_err(|error| format!("failed to render built-in AI prompt: {error}"))
}

fn effective_reply_language(settings: &AiAgentSettings) -> (&'static str, &'static str) {
    let locale = if settings
        .reply_language
        .trim()
        .eq_ignore_ascii_case(usher_i18n::AUTO_LANGUAGE)
    {
        usher_i18n::active_locale()
    } else {
        usher_i18n::resolve_preference(&settings.reply_language)
    };
    (locale.id, locale.native_name)
}

fn render_system(
    template: &PromptTemplate,
    settings: &AiAgentSettings,
    structured_output: bool,
) -> Result<(String, String), String> {
    let (language_id, language_name) = effective_reply_language(settings);
    let catalog = catalog()?;
    let language_instruction = render(
        &catalog.shared.reply_language.template,
        LanguageContext {
            reply_language_name: language_name,
            structured_output,
        },
    )?;
    let system = render(
        &template.system,
        SystemContext {
            reply_language_instruction: &language_instruction,
        },
    )?;
    Ok((system, language_id.to_string()))
}

pub(crate) fn general_chat(
    settings: &AiAgentSettings,
    history: &[CompletionMessage],
) -> Result<PromptEnvelope, String> {
    let catalog = catalog()?;
    let template = &catalog.general_chat;
    let (system, effective_reply_language) = render_system(template, settings, false)?;
    let first = history.len().saturating_sub(MAX_CONVERSATION_MESSAGES);
    let mut messages = Vec::with_capacity(history[first..].len() + 1);
    messages.push(CompletionMessage::system(system));
    messages.extend_from_slice(&history[first..]);
    Ok(PromptEnvelope {
        id: template.id.clone(),
        version: template.version,
        effective_reply_language,
        messages,
        context: vec![context_entry(
            "conversation_history",
            history[first..]
                .iter()
                .map(|message| message.content.len())
                .sum(),
            first > 0,
        )],
    })
}

#[cfg(feature = "ai-code-review")]
pub(crate) fn review_chunk(
    settings: &AiAgentSettings,
    input: &ReviewInput,
    chunk: &str,
    chunk_index: usize,
    chunk_count: usize,
) -> Result<PromptEnvelope, String> {
    let catalog = catalog()?;
    let template = &catalog.review_chunk;
    let (system, effective_reply_language) = render_system(template, settings, true)?;
    let review_rules = input
        .base_rule
        .iter()
        .chain(input.additional_rules.iter())
        .map(|rule| format!("### {}\n{}", rule.name, rule.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let target = &input.target;
    let user = render(
        &template.user,
        ReviewChunkContext {
            project_name: &target.project_name,
            pull_request_number: &target.pull_request_number,
            pull_request_title: &target.pull_request_title,
            source_branch: &target.source_branch,
            target_branch: &target.target_branch,
            base_sha: &target.base_sha,
            head_sha: &target.head_sha,
            review_rules: &review_rules,
            additional_requirements: &input.extra_requirements,
            chunk_index,
            chunk_count,
            diff_chunk: chunk,
        },
    )?;
    Ok(PromptEnvelope {
        id: template.id.clone(),
        version: template.version,
        effective_reply_language,
        messages: vec![
            CompletionMessage::system(system),
            CompletionMessage::user(user),
        ],
        context: vec![
            context_entry("pull_request_metadata", metadata_bytes(input), false),
            context_entry("review_rules", review_rules.len(), false),
            context_entry(
                "additional_requirements",
                input.extra_requirements.len(),
                false,
            ),
            context_entry("diff_chunk", chunk.len(), false),
        ],
    })
}

#[cfg(feature = "ai-code-review")]
pub(crate) fn review_conversation(
    settings: &AiAgentSettings,
    input: &ReviewInput,
    review_result: Option<&ReviewResult>,
    conversation: &[ReviewConversationMessage],
) -> Result<PromptEnvelope, String> {
    let catalog = catalog()?;
    let template = &catalog.review_conversation;
    let (system, effective_reply_language) = render_system(template, settings, false)?;
    let diff_excerpt = bounded_utf8_prefix(&input.unified_diff, MAX_REVIEW_CHAT_DIFF_BYTES);
    let completed_review = review_result
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let target = &input.target;
    let user = render(
        &template.user,
        ReviewConversationContext {
            project_name: &target.project_name,
            pull_request_number: &target.pull_request_number,
            pull_request_title: &target.pull_request_title,
            source_branch: &target.source_branch,
            target_branch: &target.target_branch,
            base_sha: &target.base_sha,
            head_sha: &target.head_sha,
            diff_excerpt,
            completed_review: &completed_review,
        },
    )?;
    let first = conversation.len().saturating_sub(MAX_CONVERSATION_MESSAGES);
    let history = &conversation[first..];
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(CompletionMessage::system(system));
    messages.push(CompletionMessage::user(user));
    messages.extend(history.iter().map(|message| match message.role {
        ReviewConversationRole::User => CompletionMessage::user(message.content.clone()),
        ReviewConversationRole::Assistant => CompletionMessage::assistant(message.content.clone()),
    }));
    Ok(PromptEnvelope {
        id: template.id.clone(),
        version: template.version,
        effective_reply_language,
        messages,
        context: vec![
            context_entry("pull_request_metadata", metadata_bytes(input), false),
            context_entry(
                "diff_excerpt",
                diff_excerpt.len(),
                input.unified_diff_truncated || diff_excerpt.len() < input.unified_diff.len(),
            ),
            context_entry("completed_review", completed_review.len(), false),
            context_entry(
                "conversation_history",
                history.iter().map(|message| message.content.len()).sum(),
                first > 0,
            ),
        ],
    })
}

#[cfg(feature = "ai-code-review")]
fn metadata_bytes(input: &ReviewInput) -> usize {
    let target = &input.target;
    [
        &target.project_name,
        &target.pull_request_number,
        &target.pull_request_title,
        &target.source_branch,
        &target.target_branch,
        &target.base_sha,
        &target.head_sha,
    ]
    .iter()
    .map(|value| value.len())
    .sum()
}

fn context_entry(name: &str, bytes: usize, truncated: bool) -> PromptContextMetadata {
    PromptContextMetadata {
        name: name.to_string(),
        bytes: bytes.min(u64::MAX as usize) as u64,
        truncated,
    }
}

#[cfg(feature = "ai-code-review")]
fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ai-code-review")]
    use std::path::PathBuf;
    #[cfg(feature = "ai-code-review")]
    use usher_ai_review::{
        AiModelRef, ProjectReviewKey, RepositoryContext, ReviewTarget, RuleId, RuleSnapshot,
    };

    fn settings(reply_language: &str) -> AiAgentSettings {
        AiAgentSettings {
            reply_language: reply_language.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn catalog_parses_and_general_chat_injects_language_once() {
        let _guard = crate::i18n::test_locale_guard();
        usher_i18n::activate_locale("en");
        let envelope = general_chat(
            &settings("zh-CN"),
            &[CompletionMessage::user("How should this be tested?")],
        )
        .unwrap();

        assert_eq!(envelope.id, "general_chat");
        assert_eq!(envelope.version, 1);
        assert_eq!(envelope.effective_reply_language, "zh-CN");
        assert!(
            envelope.messages[0]
                .content
                .contains("请用简体中文回复结果。")
        );
        assert_eq!(
            envelope.messages[0]
                .content
                .matches("请用简体中文回复结果。")
                .count(),
            1
        );
    }

    #[test]
    fn auto_reply_language_uses_the_active_interface_locale() {
        let _guard = crate::i18n::test_locale_guard();
        usher_i18n::activate_locale("zh-CN");
        let envelope = general_chat(&settings("auto"), &[]).unwrap();
        assert_eq!(envelope.effective_reply_language, "zh-CN");
        assert!(
            envelope.messages[0]
                .content
                .contains("请用简体中文回复结果。")
        );
        usher_i18n::activate_locale(usher_i18n::fallback_locale().id);
    }

    #[cfg(feature = "ai-code-review")]
    fn review_input() -> ReviewInput {
        ReviewInput {
            target: ReviewTarget {
                project_key: ProjectReviewKey("github:usher".into()),
                project_name: "Usher".into(),
                pull_request_number: "42".into(),
                pull_request_title: "Centralize prompts".into(),
                base_sha: "base-sha".into(),
                head_sha: "head-sha".into(),
                author: "private-author".into(),
                source_branch: "feature/prompts".into(),
                target_branch: "main".into(),
                updated_at: "private-updated-at".into(),
            },
            model: AiModelRef {
                provider: "fixture".into(),
                base_url: "https://provider.invalid".into(),
                model: "fixture".into(),
            },
            unified_diff: "diff --git a/src/lib.rs b/src/lib.rs\n+approved_context();".into(),
            unified_diff_truncated: false,
            repository_context: RepositoryContext {
                local_repo: Some(PathBuf::from("/Users/private/repository")),
                remote_repo: Some("https://secret.example/repository.git".into()),
            },
            base_rule: Some(RuleSnapshot {
                id: RuleId("default".into()),
                name: "Correctness".into(),
                markdown_path: PathBuf::from("/Users/private/rules.md"),
                content: "Report concrete defects.".into(),
                content_hash: "secret-hash".into(),
            }),
            additional_rules: Vec::new(),
            extra_requirements: "Focus on regressions.".into(),
        }
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_prompt_injects_language_once_and_preserves_json_protocol() {
        let envelope = review_chunk(
            &settings("zh-CN"),
            &review_input(),
            "+approved_context();",
            1,
            1,
        )
        .unwrap();
        let system = &envelope.messages[0].content;

        assert_eq!(system.matches("请用简体中文回复结果。").count(), 1);
        for protocol_name in [
            "summary",
            "findings",
            "skipped_files",
            "severity",
            "high|medium|low",
            "file",
            "line",
            "side",
            "left|right",
            "title",
            "explanation",
            "suggestion",
        ] {
            assert!(system.contains(protocol_name), "missing {protocol_name}");
        }
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_prompt_has_only_the_approved_context_fields() {
        let input = review_input();
        let envelope = review_chunk(&settings("en"), &input, &input.unified_diff, 1, 1).unwrap();
        let request = &envelope.messages[1].content;

        for expected in [
            "Usher",
            "#42 Centralize prompts",
            "feature/prompts -> main",
            "base-sha",
            "head-sha",
            "Correctness",
            "Report concrete defects.",
            "Focus on regressions.",
            "+approved_context();",
        ] {
            assert!(request.contains(expected), "missing {expected}");
        }
        for excluded in [
            "/Users/private/repository",
            "https://secret.example/repository.git",
            "/Users/private/rules.md",
            "private-author",
            "private-updated-at",
            "secret-hash",
            "https://provider.invalid",
        ] {
            assert!(!request.contains(excluded), "leaked {excluded}");
        }
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_conversation_bounds_history_and_records_truncation() {
        let mut input = review_input();
        input.unified_diff = format!("{}中", "x".repeat(MAX_REVIEW_CHAT_DIFF_BYTES));
        let conversation = (0..25)
            .map(|index| ReviewConversationMessage {
                role: if index % 2 == 0 {
                    ReviewConversationRole::User
                } else {
                    ReviewConversationRole::Assistant
                },
                content: format!("history-{index}"),
                created_at_unix_ms: index,
                elapsed_ms: None,
                prompt: None,
            })
            .collect::<Vec<_>>();

        let envelope =
            review_conversation(&settings("zh-CN"), &input, None, conversation.as_slice()).unwrap();
        let provenance = envelope.provenance(1);

        assert_eq!(envelope.messages.len(), MAX_CONVERSATION_MESSAGES + 2);
        assert_eq!(envelope.messages[0].role, "system");
        assert_eq!(envelope.messages[1].role, "user");
        assert_eq!(envelope.messages[2].content, "history-5");
        assert_eq!(provenance.effective_reply_language, "zh-CN");
        assert_eq!(provenance.request_count, 1);
        let diff = provenance
            .context
            .iter()
            .find(|entry| entry.name == "diff_excerpt")
            .unwrap();
        assert!(diff.truncated);
        assert!(diff.bytes <= MAX_REVIEW_CHAT_DIFF_BYTES as u64);
        let history = provenance
            .context
            .iter()
            .find(|entry| entry.name == "conversation_history")
            .unwrap();
        assert!(history.truncated);
    }

    #[test]
    fn strict_rendering_rejects_missing_variables() {
        let error = render("{{ missing_value }}", serde_json::json!({})).unwrap_err();
        assert!(error.contains("failed to render built-in AI prompt"));
    }

    #[tokio::test]
    #[ignore = "requires an explicitly enabled, configured real AI provider"]
    async fn live_provider_honors_auto_and_explicit_reply_languages() {
        assert_eq!(
            std::env::var("USHER_RUN_LIVE_AI_SMOKE").as_deref(),
            Ok("1"),
            "set USHER_RUN_LIVE_AI_SMOKE=1 to authorize a billable provider smoke test"
        );

        let _guard = crate::i18n::test_locale_guard();
        let mut settings = crate::settings::load_settings().ai.agent;

        usher_i18n::activate_locale("zh-CN");
        settings.reply_language = usher_i18n::AUTO_LANGUAGE.to_string();
        let auto = general_chat(
            &settings,
            &[CompletionMessage::user(
                "State the primary purpose of a mutex in exactly one short sentence.",
            )],
        )
        .expect("render auto-language prompt");
        assert_eq!(auto.effective_reply_language, "zh-CN");
        let chinese = crate::ai::agent::complete(
            &settings,
            auto.messages.as_slice(),
            "Live AI reply-language smoke test",
        )
        .await
        .expect("complete auto-language request");
        assert!(
            contains_cjk(&chinese),
            "auto-language reply was not Chinese"
        );

        settings.reply_language = "en".to_string();
        let explicit = general_chat(
            &settings,
            &[CompletionMessage::user(
                "用一个很短的句子说明互斥锁的主要用途。",
            )],
        )
        .expect("render explicit-language prompt");
        assert_eq!(explicit.effective_reply_language, "en");
        let english = crate::ai::agent::complete(
            &settings,
            explicit.messages.as_slice(),
            "Live AI reply-language smoke test",
        )
        .await
        .expect("complete explicit-language request");
        assert!(
            english
                .chars()
                .any(|character| character.is_ascii_alphabetic()),
            "explicit English reply did not contain English text"
        );
        assert!(
            !contains_cjk(&english),
            "explicit English reply contained Chinese text"
        );

        usher_i18n::activate_locale(usher_i18n::fallback_locale().id);
    }

    fn contains_cjk(value: &str) -> bool {
        value
            .chars()
            .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
    }
}
