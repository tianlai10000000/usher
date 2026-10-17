//! Application facade for locale selection and Slint translation state.
//!
//! Catalog parsing, locale aliases, and fallback policy belong to
//! `usher-i18n`. This facade owns the application-specific Slint bridge so the
//! domain crate never depends on a concrete generated UI engine.

use anyhow::{Context, Result};
use slint::Global;
use std::borrow::Cow;
use std::fmt;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use crate::{AppState, LanguageOption, MainWindow};

#[cfg(test)]
static TEST_LOCALE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_locale_guard() -> MutexGuard<'static, ()> {
    TEST_LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// User-facing copy that stays semantic until it reaches a presentation
/// boundary. Long-lived workflow state stores this type instead of freezing a
/// translation into a `String`, so changing locale can re-project existing
/// state without reconstructing or matching its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalizedText {
    Literal(String),
    Message {
        key: &'static str,
        arguments: Vec<(&'static str, String)>,
    },
}

impl LocalizedText {
    pub(crate) fn empty() -> Self {
        Self::Literal(String::new())
    }

    pub(crate) fn message(key: &'static str) -> Self {
        Self::Message {
            key,
            arguments: Vec::new(),
        }
    }

    pub(crate) fn message_with<const N: usize>(
        key: &'static str,
        arguments: [(&'static str, String); N],
    ) -> Self {
        Self::Message {
            key,
            arguments: arguments.into(),
        }
    }

    pub(crate) fn resolve(&self) -> String {
        self.resolve_for(usher_i18n::active_locale().id)
    }

    pub(crate) fn source_text(&self) -> String {
        self.resolve_for(usher_i18n::source_locale().id)
    }

    fn resolve_for(&self, locale: &str) -> String {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Message { key, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect::<Vec<_>>();
                usher_i18n::translate_for_with(locale, key, arguments.as_slice())
            }
        }
    }
}

impl Default for LocalizedText {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<String> for LocalizedText {
    fn from(value: String) -> Self {
        Self::Literal(value)
    }
}

impl From<&str> for LocalizedText {
    fn from(value: &str) -> Self {
        Self::Literal(value.to_string())
    }
}

impl fmt::Display for LocalizedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source_text().as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageChoice {
    pub id: String,
    pub label: String,
}

pub(crate) fn initialize(app: &MainWindow, preference: &str) -> Result<()> {
    apply_preference(preference).context("Failed to select the initial UI language")?;
    sync_main_window(app, preference);
    Ok(())
}

pub(crate) fn apply_preference(preference: &str) -> Result<&'static str> {
    let locale = usher_i18n::resolve_preference(preference);
    slint::select_bundled_translation(locale.id)
        .with_context(|| format!("Failed to select bundled locale {}", locale.id))?;
    // ComponentFactory products are independently generated UI roots. Force
    // every root to re-evaluate @tr bindings after changing the bundle index.
    i_slint_core::translations::mark_all_translations_dirty();
    usher_i18n::activate_locale(locale.id)
        .with_context(|| format!("Resolved locale {} is not registered", locale.id))?;
    Ok(usher_i18n::canonical_preference(preference))
}

pub(crate) fn sync_main_window(app: &MainWindow, preference: &str) {
    let state = AppState::get(app);
    let reply_language = state.get_ai_agent_reply_language_selection().id.to_string();
    let locale = usher_i18n::resolve_preference(preference);
    let canonical = usher_i18n::canonical_preference(preference);
    let choices = language_choices();
    let selected_label = choices
        .iter()
        .find(|choice| choice.id == canonical)
        .map(|choice| choice.label.as_str())
        .unwrap_or(canonical);
    state.set_language_selection(LanguageOption {
        id: canonical.into(),
        label: selected_label.into(),
    });
    app.set_workflow_pr_list_compact_text_offset_y(locale.pr_list_compact_text_offset_y);
    sync_ai_reply_language(app, &reply_language);
}

pub(crate) fn language_choices() -> Vec<LanguageChoice> {
    let mut choices = Vec::with_capacity(usher_i18n::available_locales().len() + 1);
    choices.push(LanguageChoice {
        id: usher_i18n::AUTO_LANGUAGE.to_string(),
        label: usher_i18n::translate("language.system_default").into_owned(),
    });
    choices.extend(
        usher_i18n::available_locales()
            .iter()
            .map(|locale| LanguageChoice {
                id: locale.id.to_string(),
                label: locale.native_name.to_string(),
            }),
    );
    choices
}

pub(crate) fn ai_reply_language_choices() -> Vec<LanguageChoice> {
    let mut choices = Vec::with_capacity(usher_i18n::available_locales().len() + 1);
    choices.push(LanguageChoice {
        id: usher_i18n::AUTO_LANGUAGE.to_string(),
        label: usher_i18n::translate("settings.ai_agent.reply_language_auto").into_owned(),
    });
    choices.extend(
        usher_i18n::available_locales()
            .iter()
            .map(|locale| LanguageChoice {
                id: locale.id.to_string(),
                label: locale.native_name.to_string(),
            }),
    );
    choices
}

pub(crate) fn sync_ai_reply_language(app: &MainWindow, preference: &str) {
    let state = AppState::get(app);
    let canonical = if preference
        .trim()
        .eq_ignore_ascii_case(usher_i18n::AUTO_LANGUAGE)
    {
        usher_i18n::AUTO_LANGUAGE
    } else {
        usher_i18n::canonical_preference(preference)
    };
    let choices = ai_reply_language_choices();
    let selected_label = choices
        .iter()
        .find(|choice| choice.id == canonical)
        .map(|choice| choice.label.as_str())
        .unwrap_or(canonical);
    state.set_ai_agent_reply_language_selection(LanguageOption {
        id: canonical.into(),
        label: selected_label.into(),
    });
}

pub(crate) fn tr(key: &str) -> Cow<'_, str> {
    usher_i18n::translate(key)
}

pub(crate) fn tr_args(key: &str, arguments: &[(&str, &str)]) -> String {
    usher_i18n::translate_with(key, arguments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_backend_testing::ElementHandle;
    use slint::ComponentHandle;
    use usher_messages::AppState as MessagesAppState;
    use usher_ui_settings::{AppState as SettingsAppState, GitHubSettingsPage};

    #[test]
    fn language_choices_are_manifest_driven_and_include_system_default() {
        let _locale_guard = test_locale_guard();
        usher_i18n::activate_preference("en");
        let choices = language_choices();
        assert_eq!(choices[0].id, usher_i18n::AUTO_LANGUAGE);
        assert_eq!(choices.len(), usher_i18n::available_locales().len() + 1);
        for locale in usher_i18n::available_locales() {
            assert!(choices.iter().any(|choice| choice.id == locale.id));
        }
    }

    #[test]
    fn ai_reply_language_choices_use_a_distinct_auto_label() {
        let _locale_guard = test_locale_guard();
        usher_i18n::activate_preference("en");
        let choices = ai_reply_language_choices();
        assert_eq!(choices[0].id, usher_i18n::AUTO_LANGUAGE);
        assert_eq!(choices[0].label, "Follow interface language");
        assert_eq!(choices.len(), usher_i18n::available_locales().len() + 1);
    }

    #[test]
    fn localized_text_resolves_against_the_current_catalog() {
        let _locale_guard = test_locale_guard();
        let text = LocalizedText::message_with(
            "update.latest_message",
            [("version", "v1.2.3".to_string())],
        );
        usher_i18n::activate_preference("en");
        assert_eq!(text.resolve(), "Usher v1.2.3 is the latest version.");
        usher_i18n::activate_preference("zh-CN");
        assert_eq!(text.resolve(), "Usher v1.2.3 已是最新版本。");
        usher_i18n::activate_preference(usher_i18n::fallback_locale().id);
    }

    #[test]
    fn independent_ui_roots_share_the_same_translation_indexes() {
        let _locale_guard = test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().expect("main window");
        let settings = GitHubSettingsPage::new().expect("settings page");
        let messages = usher_messages::GitHubMessagesPage::new().expect("messages page");
        // Static builds resolve a separate global graph on the same actual
        // WindowAdapter. Integrated preview resolves the existing MainWindow;
        // neither path reconstructs PR data models for locale changes.
        let review = crate::pr_review_fixture_view(&window);
        let assert_review_catalog = |locale: &str| {
            let expected = usher_i18n::translate_for(locale, "language.title");
            #[cfg(not(feature = "live-preview"))]
            assert_eq!(
                review
                    .global::<usher_ui_pr_review::AppState>()
                    .get_i18n_catalog_sentinel(),
                expected,
                "PR Review factory must retain the complete UI catalog ordering"
            );
            #[cfg(feature = "live-preview")]
            assert_eq!(AppState::get(&review).get_i18n_catalog_sentinel(), expected);
            let state = review.global::<crate::PrReviewBridge>();
            assert_eq!(
                state.get_workflow_pr_review_commit_range_label(),
                usher_i18n::translate_for(locale, "pr_review.all_commits"),
                "PR Review global copy must refresh at the same language index"
            );
            assert_eq!(
                state.get_workflow_pr_revert_action_label(),
                usher_i18n::translate_for(locale, "pr_review.revert_pull_request")
            );
        };
        #[cfg(all(feature = "ui-components", not(feature = "live-preview")))]
        let gallery = usher_ui_components::GitHubUiComponentsWindow::new().expect("gallery page");

        apply_preference(usher_i18n::source_locale().id).expect("select source language");
        assert_review_catalog(usher_i18n::source_locale().id);
        #[cfg(all(feature = "ui-components", not(feature = "live-preview")))]
        assert_eq!(
            gallery
                .global::<usher_ui_components::AppState>()
                .get_i18n_catalog_sentinel(),
            usher_i18n::translate_for(usher_i18n::source_locale().id, "language.title")
        );
        sync_main_window(&window, usher_i18n::source_locale().id);
        assert_eq!(
            window.get_workflow_pr_list_compact_text_offset_y(),
            usher_i18n::source_locale().pr_list_compact_text_offset_y
        );
        window.set_current_route(crate::Route::Workflow);
        for label in ["Pull requests", "Tags", "Branch", "CD"] {
            assert!(
                ElementHandle::find_by_accessible_label(&window, label)
                    .next()
                    .is_some(),
                "source-language Workflow navigation must retain {label:?}"
            );
        }

        for locale in usher_i18n::available_locales()
            .iter()
            .filter(|locale| locale.id != usher_i18n::source_locale().id)
        {
            apply_preference(locale.id).expect("select packaged locale");
            assert_review_catalog(locale.id);
            sync_main_window(&window, locale.id);
            assert_eq!(
                window.get_workflow_pr_list_compact_text_offset_y(),
                locale.pr_list_compact_text_offset_y,
                "runtime locale switch must refresh PR-list typography metadata"
            );
            let expected = usher_i18n::translate_for(locale.id, "language.title");
            assert_eq!(AppState::get(&window).get_i18n_catalog_sentinel(), expected);
            assert_eq!(
                SettingsAppState::get(&settings).get_i18n_catalog_sentinel(),
                expected
            );
            assert_eq!(
                MessagesAppState::get(&messages).get_i18n_catalog_sentinel(),
                expected
            );

            #[cfg(all(feature = "ui-components", not(feature = "live-preview")))]
            assert_eq!(
                gallery
                    .global::<usher_ui_components::AppState>()
                    .get_i18n_catalog_sentinel(),
                expected,
                "gallery factory must share the complete UI catalog ordering"
            );
            let settings_title = usher_i18n::translate_for(locale.id, "settings.title");
            assert!(
                ElementHandle::find_by_accessible_label(&settings, settings_title.as_ref())
                    .next()
                    .is_some(),
                "runtime locale switch must repaint the Settings factory title"
            );

            for key in ["navigation.home", "common.projects", "workflow.sections"] {
                let label = usher_i18n::translate_for(locale.id, key);
                assert!(
                    ElementHandle::find_by_accessible_label(&window, label.as_ref())
                        .next()
                        .is_some(),
                    "runtime locale switch must repaint visible MainWindow copy for {key:?}"
                );
            }
        }

        apply_preference(usher_i18n::source_locale().id).expect("restore source language");
        assert_review_catalog(usher_i18n::source_locale().id);
        sync_main_window(&window, usher_i18n::source_locale().id);
    }
}
