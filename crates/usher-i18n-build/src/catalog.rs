use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use rspolib::pofile;

use crate::validation::{
    Manifest, MessageManifest, MessageSpec, message_placeholders, validate_messages,
};
use crate::{BuildResult, watch};

#[derive(Clone, Copy)]
pub(crate) enum Scope {
    Slint,
    Rust,
}

impl Scope {
    fn registry(self) -> &'static str {
        match self {
            Self::Slint => "messages.toml",
            Self::Rust => "rust-messages.toml",
        }
    }

    fn po_name(self) -> &'static str {
        match self {
            Self::Slint => "usher.po",
            Self::Rust => "usher-rust.po",
        }
    }
}

pub(crate) struct LocaleCatalog {
    pub id: String,
    pub values: BTreeMap<String, String>,
    pub translated: usize,
    pub po_source: Option<String>,
}

pub(crate) fn load_messages(i18n_root: &Path, scope: Scope) -> BuildResult<Vec<MessageSpec>> {
    let path = i18n_root.join(scope.registry());
    watch(&path);
    let manifest: MessageManifest = toml::from_str(&fs::read_to_string(&path)?)?;
    let slint_visible = matches!(scope, Scope::Slint);
    if slint_visible && manifest.messages.is_empty() {
        return Err("Slint i18n message catalog must not be empty".into());
    }
    for message in &manifest.messages {
        if message.slint != slint_visible {
            return Err(format!(
                "{} contains misplaced message {:?}: expected slint = {slint_visible}",
                path.display(),
                message.key
            )
            .into());
        }
    }
    validate_messages(manifest.messages)
}

pub(crate) fn load_catalogs(
    i18n_root: &Path,
    manifest: &Manifest,
    messages: &[MessageSpec],
    scope: Scope,
) -> BuildResult<Vec<LocaleCatalog>> {
    manifest
        .locales
        .iter()
        .map(|locale| {
            if locale.id == manifest.source_locale {
                return Ok(LocaleCatalog {
                    id: locale.id.clone(),
                    values: messages
                        .iter()
                        .map(|message| (message.key.clone(), message.text.clone()))
                        .collect(),
                    translated: messages.len(),
                    po_source: None,
                });
            }
            let path = i18n_root
                .join("locales")
                .join(&locale.id)
                .join("LC_MESSAGES")
                .join(scope.po_name());
            watch(&path);
            load_po(&path, &locale.id, messages)
        })
        .collect()
}

fn load_po(path: &Path, locale_id: &str, messages: &[MessageSpec]) -> BuildResult<LocaleCatalog> {
    let po_source = fs::read_to_string(path)?;
    let po = pofile(path)?;
    let known_messages = messages
        .iter()
        .map(|message| (message.key.as_str(), message))
        .collect::<BTreeMap<_, _>>();
    let mut seen_contexts = BTreeSet::new();
    let mut active_entries = BTreeMap::new();
    for entry in po
        .entries
        .iter()
        .filter(|entry| !entry.msgid.is_empty() && !entry.obsolete)
    {
        let context = entry.msgctxt.as_deref().unwrap_or_default();
        let Some(message) = known_messages.get(context) else {
            return Err(format!(
                "{} contains unknown message context {context:?}",
                path.display()
            )
            .into());
        };
        if !seen_contexts.insert(context) {
            return Err(format!(
                "{} contains duplicate message context {context:?}",
                path.display()
            )
            .into());
        }
        if entry.msgid != message.text {
            return Err(format!(
                "{} contains source-text drift for {context:?}: expected {:?}, found {:?}",
                path.display(),
                message.text,
                entry.msgid
            )
            .into());
        }
        active_entries.insert(context, entry);
    }
    let mut translated = 0;
    let mut values = BTreeMap::new();
    for message in messages {
        let value = active_entries
            .get(message.key.as_str())
            .filter(|entry| !entry.flags.iter().any(|flag| flag == "fuzzy"))
            .and_then(|entry| entry.msgstr.as_deref())
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            let expected = message_placeholders(&message.text)?;
            let actual = message_placeholders(value).map_err(|error| {
                format!(
                    "{} contains invalid translated placeholders for {:?}: {error}",
                    path.display(),
                    message.key
                )
            })?;
            if actual != expected {
                return Err(format!(
                    "{} contains placeholder drift for {:?}: expected {:?}, found {:?}",
                    path.display(),
                    message.key,
                    expected,
                    actual
                )
                .into());
            }
            translated += 1;
        }
        values.insert(
            message.key.clone(),
            value.unwrap_or(&message.text).to_string(),
        );
    }
    Ok(LocaleCatalog {
        id: locale_id.to_owned(),
        values,
        translated,
        po_source: Some(po_source),
    })
}
