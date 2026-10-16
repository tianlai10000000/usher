use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::catalog::LocaleCatalog;
use crate::{BuildResult, watch};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub source_locale: String,
    pub fallback_locale: String,
    pub locales: Vec<LocaleSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleSpec {
    pub id: String,
    pub native_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub minimum_coverage: f64,
    #[serde(default)]
    pub segmented_count_suffix_offset: i32,
    #[serde(default)]
    pub pr_list_compact_text_offset_y: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageManifest {
    pub messages: Vec<MessageSpec>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageSpec {
    pub key: String,
    pub text: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default = "default_slint_visibility")]
    pub slint: bool,
}

fn default_slint_visibility() -> bool {
    true
}

pub(crate) fn load_manifest(i18n_root: &Path) -> BuildResult<Manifest> {
    let path = i18n_root.join("manifest.toml");
    watch(&path);
    let manifest = toml::from_str(&fs::read_to_string(path)?)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> BuildResult<()> {
    if manifest.locales.is_empty() {
        return Err("i18n manifest must declare at least one locale".into());
    }
    if manifest.source_locale != "en" {
        return Err("Slint bundled translations require the source locale id to be 'en'".into());
    }
    let mut identifiers = BTreeSet::new();
    for locale in &manifest.locales {
        if locale.native_name.trim().is_empty() {
            return Err(format!("locale id and native_name must be non-empty: {locale:?}").into());
        }
        if !(0.0..=1.0).contains(&locale.minimum_coverage) {
            return Err(format!("invalid minimum_coverage for {}", locale.id).into());
        }
        if !(0..=2).contains(&locale.segmented_count_suffix_offset) {
            return Err(format!(
                "invalid segmented_count_suffix_offset for {}: {}",
                locale.id, locale.segmented_count_suffix_offset
            )
            .into());
        }
        if !(-2..=2).contains(&locale.pr_list_compact_text_offset_y) {
            return Err(format!(
                "invalid pr_list_compact_text_offset_y for {}: {}",
                locale.id, locale.pr_list_compact_text_offset_y
            )
            .into());
        }
        for identifier in std::iter::once(&locale.id).chain(locale.aliases.iter()) {
            if !is_locale_identifier(identifier) {
                return Err(format!(
                    "locale id or alias must use non-empty ASCII alphanumeric segments: {identifier:?}"
                )
                .into());
            }
            let normalized = identifier.trim().replace('_', "-").to_ascii_lowercase();
            if !identifiers.insert(normalized.clone()) {
                return Err(
                    format!("duplicate locale id or alias: {identifier} ({normalized})").into(),
                );
            }
        }
    }
    for required in [&manifest.source_locale, &manifest.fallback_locale] {
        if !manifest.locales.iter().any(|locale| &locale.id == required) {
            return Err(format!("manifest locale {required:?} is not declared").into());
        }
    }
    Ok(())
}

pub(crate) fn validate_messages(mut messages: Vec<MessageSpec>) -> BuildResult<Vec<MessageSpec>> {
    let mut keys = BTreeSet::new();
    let mut property_names = BTreeSet::new();
    for message in &messages {
        if message.key.trim().is_empty() || message.text.is_empty() {
            return Err(format!("message key and text must be non-empty: {}", message.key).into());
        }
        if !message
            .key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(format!(
                "message key contains unsupported characters: {}",
                message.key
            )
            .into());
        }
        if !keys.insert(message.key.clone()) {
            return Err(format!("duplicate message key: {}", message.key).into());
        }
        message_placeholders(&message.text).map_err(|error| {
            format!("invalid placeholders for message {}: {error}", message.key)
        })?;
        let property_name = slint_property_name(&message.key);
        if !property_names.insert(property_name.clone()) {
            return Err(format!("message keys collide as Slint property {property_name:?}").into());
        }
    }
    messages.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(messages)
}

pub(crate) fn validate_coverage(
    manifest: &Manifest,
    catalogs: &[LocaleCatalog],
    message_count: usize,
) -> BuildResult<()> {
    for (locale, catalog) in manifest.locales.iter().zip(catalogs) {
        let coverage = catalog.translated as f64 / message_count as f64;
        if coverage + f64::EPSILON < locale.minimum_coverage {
            return Err(format!(
                "{} translation coverage {:.1}% is below the configured {:.1}% minimum",
                locale.id,
                coverage * 100.0,
                locale.minimum_coverage * 100.0
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn message_placeholders(text: &str) -> Result<BTreeSet<String>, String> {
    let mut placeholders = BTreeSet::new();
    let mut remainder = text;
    while let Some(open_index) = remainder.find('{') {
        if remainder[..open_index].contains('}') {
            return Err("closing brace appears before an opening brace".to_string());
        }
        let placeholder = &remainder[open_index + 1..];
        let Some(close_index) = placeholder.find('}') else {
            return Err("opening brace has no matching closing brace".to_string());
        };
        let name = &placeholder[..close_index];
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(format!("unsupported placeholder name {name:?}"));
        }
        placeholders.insert(name.to_string());
        remainder = &placeholder[close_index + 1..];
    }
    if remainder.contains('}') {
        return Err("closing brace has no matching opening brace".to_string());
    }
    Ok(placeholders)
}

pub(crate) fn slint_property_name(key: &str) -> String {
    key.chars()
        .map(|character| match character {
            '.' | '_' => '-',
            other => other,
        })
        .collect()
}

fn is_locale_identifier(locale: &str) -> bool {
    !locale.is_empty()
        && locale.split(['-', '_']).all(|segment| {
            !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}
