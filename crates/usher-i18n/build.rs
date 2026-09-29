use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use rspolib::pofile;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    source_locale: String,
    fallback_locale: String,
    locales: Vec<LocaleSpec>,
}

#[derive(Debug, Deserialize)]
struct LocaleSpec {
    id: String,
    native_name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    minimum_coverage: f64,
    #[serde(default)]
    segmented_count_suffix_offset: i32,
    #[serde(default)]
    pr_list_compact_text_offset_y: i32,
}

#[derive(Debug, Deserialize)]
struct MessageManifest {
    messages: Vec<MessageSpec>,
}

#[derive(Debug, Deserialize, Clone)]
struct MessageSpec {
    key: String,
    text: String,
    #[serde(default)]
    comment: String,
    #[serde(default = "default_slint_visibility")]
    slint: bool,
}

fn default_slint_visibility() -> bool {
    true
}

struct LocaleCatalog {
    id: String,
    values: BTreeMap<String, String>,
    po_path: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let i18n_root = repository_root.join("i18n");
    let manifest_path = i18n_root.join("manifest.toml");
    let messages_path = i18n_root.join("messages.toml");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", messages_path.display());

    let manifest: Manifest = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
    let message_manifest: MessageManifest = toml::from_str(&fs::read_to_string(&messages_path)?)?;
    validate_manifest(&manifest)?;
    let messages = validate_messages(message_manifest.messages)?;

    let mut catalogs = Vec::with_capacity(manifest.locales.len());
    for locale in &manifest.locales {
        let catalog = load_locale_catalog(&i18n_root, &manifest.source_locale, locale, &messages)?;
        if let Some(path) = &catalog.po_path {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        catalogs.push(catalog);
    }

    let generated = generate_rust(&manifest, &messages, &catalogs)?;
    fs::write(
        PathBuf::from(std::env::var("OUT_DIR")?).join("catalogs.rs"),
        generated,
    )?;
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<(), Box<dyn Error>> {
    if manifest.locales.is_empty() {
        return Err("i18n manifest must declare at least one locale".into());
    }
    if manifest.source_locale != "en" {
        return Err("Slint bundled translations require the source locale id to be 'en'".into());
    }

    let mut identifiers = BTreeSet::new();
    for locale in &manifest.locales {
        if locale.id.trim().is_empty() || locale.native_name.trim().is_empty() {
            return Err(format!("locale id and native_name must be non-empty: {locale:?}").into());
        }
        if !is_locale_identifier(&locale.id) {
            return Err(format!(
                "locale id must be a path-safe BCP-47-like identifier: {:?}",
                locale.id
            )
            .into());
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
            let normalized = normalize_locale(identifier);
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

fn validate_messages(mut messages: Vec<MessageSpec>) -> Result<Vec<MessageSpec>, Box<dyn Error>> {
    if messages.is_empty() {
        return Err("i18n message catalog must not be empty".into());
    }
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

fn load_locale_catalog(
    i18n_root: &Path,
    source_locale: &str,
    locale: &LocaleSpec,
    messages: &[MessageSpec],
) -> Result<LocaleCatalog, Box<dyn Error>> {
    if locale.id == source_locale {
        return Ok(LocaleCatalog {
            id: locale.id.clone(),
            values: messages
                .iter()
                .map(|message| (message.key.clone(), message.text.clone()))
                .collect(),
            po_path: None,
        });
    }

    let po_path = i18n_root
        .join("locales")
        .join(&locale.id)
        .join("LC_MESSAGES")
        .join("usher.po");
    let po = pofile(po_path.as_path())?;
    let known_messages = messages
        .iter()
        .map(|message| (message.key.as_str(), message))
        .collect::<BTreeMap<_, _>>();
    let mut seen_contexts = BTreeSet::new();
    for entry in po
        .entries
        .iter()
        .filter(|entry| !entry.msgid.is_empty() && !entry.obsolete)
    {
        let context = entry.msgctxt.as_deref().unwrap_or_default();
        let Some(message) = known_messages.get(context) else {
            return Err(format!(
                "{} contains unknown message context {context:?}",
                po_path.display()
            )
            .into());
        };
        if !seen_contexts.insert(context) {
            return Err(format!(
                "{} contains duplicate message context {context:?}",
                po_path.display()
            )
            .into());
        }
        if entry.msgid != message.text {
            return Err(format!(
                "{} contains source-text drift for {context:?}: expected {:?}, found {:?}",
                po_path.display(),
                message.text,
                entry.msgid
            )
            .into());
        }
    }

    let mut translated = 0usize;
    let mut values = BTreeMap::new();
    for message in messages {
        let entry = po.find_by_msgid_msgctxt(&message.text, &message.key);
        let value = entry
            .as_ref()
            .filter(|entry| !entry.flags.iter().any(|flag| flag == "fuzzy"))
            .and_then(|entry| entry.msgstr.as_deref())
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            let source_placeholders = message_placeholders(&message.text).map_err(|error| {
                format!("invalid source placeholders for {}: {error}", message.key)
            })?;
            let translated_placeholders = message_placeholders(value).map_err(|error| {
                format!(
                    "{} contains invalid translated placeholders for {:?}: {error}",
                    po_path.display(),
                    message.key
                )
            })?;
            if translated_placeholders != source_placeholders {
                return Err(format!(
                    "{} contains placeholder drift for {:?}: expected {:?}, found {:?}",
                    po_path.display(),
                    message.key,
                    source_placeholders,
                    translated_placeholders
                )
                .into());
            }
        }
        if value.is_some() {
            translated += 1;
        }
        values.insert(
            message.key.clone(),
            value.unwrap_or(&message.text).to_string(),
        );
    }

    let coverage = translated as f64 / messages.len() as f64;
    if coverage + f64::EPSILON < locale.minimum_coverage {
        return Err(format!(
            "{} translation coverage {:.1}% is below the configured {:.1}% minimum",
            locale.id,
            coverage * 100.0,
            locale.minimum_coverage * 100.0
        )
        .into());
    }

    Ok(LocaleCatalog {
        id: locale.id.clone(),
        values,
        po_path: Some(po_path),
    })
}

fn message_placeholders(text: &str) -> Result<BTreeSet<String>, String> {
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

fn generate_rust(
    manifest: &Manifest,
    messages: &[MessageSpec],
    catalogs: &[LocaleCatalog],
) -> Result<String, Box<dyn Error>> {
    let source_index = manifest
        .locales
        .iter()
        .position(|locale| locale.id == manifest.source_locale)
        .ok_or("source locale disappeared during generation")?;
    let fallback_index = manifest
        .locales
        .iter()
        .position(|locale| locale.id == manifest.fallback_locale)
        .ok_or("fallback locale disappeared during generation")?;

    let mut output = String::new();
    writeln!(
        output,
        "const GENERATED_SOURCE_LOCALE_INDEX: usize = {source_index};"
    )?;
    writeln!(
        output,
        "const GENERATED_FALLBACK_LOCALE_INDEX: usize = {fallback_index};"
    )?;
    writeln!(output, "static GENERATED_LOCALES: &[LocaleMetadata] = &[")?;
    for locale in &manifest.locales {
        writeln!(
            output,
            "    LocaleMetadata {{ id: {:?}, native_name: {:?}, aliases: &{:?}, segmented_count_suffix_offset: {}, pr_list_compact_text_offset_y: {} }},",
            locale.id,
            locale.native_name,
            locale.aliases,
            locale.segmented_count_suffix_offset,
            locale.pr_list_compact_text_offset_y
        )?;
    }
    writeln!(output, "];\n")?;

    for (index, catalog) in catalogs.iter().enumerate() {
        writeln!(output, "static CATALOG_{index}: &[CatalogEntry] = &[")?;
        for (key, value) in &catalog.values {
            writeln!(
                output,
                "    CatalogEntry {{ key: {:?}, value: {:?} }},",
                key, value
            )?;
        }
        writeln!(output, "];\n")?;
    }
    writeln!(output, "static GENERATED_CATALOGS: &[&[CatalogEntry]] = &[")?;
    for index in 0..catalogs.len() {
        writeln!(output, "    CATALOG_{index},")?;
    }
    writeln!(output, "];\n")?;

    writeln!(output, "static GENERATED_PO_SOURCES: &[(&str, &str)] = &[")?;
    for catalog in catalogs {
        if let Some(path) = &catalog.po_path {
            writeln!(
                output,
                "    ({:?}, include_str!({:?})),",
                catalog.id,
                path.to_string_lossy()
            )?;
        }
    }
    writeln!(output, "];\n")?;

    let slint_catalog = generate_slint_catalog(messages)?;
    writeln!(
        output,
        "static GENERATED_SLINT_CATALOG: &str = {:?};",
        slint_catalog
    )?;
    Ok(output)
}

fn generate_slint_catalog(messages: &[MessageSpec]) -> Result<String, Box<dyn Error>> {
    let mut slint = String::from(
        "// Generated from i18n/messages.toml. Do not edit.\n\nexport global I18n {\n",
    );
    for message in messages.iter().filter(|message| message.slint) {
        if !message.comment.is_empty() {
            writeln!(slint, "    // {}", message.comment.replace('\n', " "))?;
        }
        let slint_text = message.text.replace('{', "{{").replace('}', "}}");
        writeln!(
            slint,
            "    out property <string> {}: @tr({:?} => {:?});",
            slint_property_name(&message.key),
            message.key,
            slint_text
        )?;
    }
    slint.push_str("}\n");
    Ok(slint)
}

fn slint_property_name(key: &str) -> String {
    key.chars()
        .map(|character| match character {
            '.' | '_' => '-',
            other => other,
        })
        .collect()
}

fn normalize_locale(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
}

fn is_locale_identifier(locale: &str) -> bool {
    !locale.is_empty()
        && locale.split(['-', '_']).all(|segment| {
            !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}
