use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocaleMetadata {
    pub id: &'static str,
    pub native_name: &'static str,
    pub aliases: &'static [&'static str],
    pub segmented_count_suffix_offset: i32,
    pub pr_list_compact_text_offset_y: i32,
}

#[derive(Debug, Clone, Copy)]
struct CatalogEntry {
    key: &'static str,
    value: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

pub const AUTO_LANGUAGE: &str = "auto";

static ACTIVE_LOCALE_INDEX: AtomicUsize = AtomicUsize::new(GENERATED_FALLBACK_LOCALE_INDEX);

pub fn available_locales() -> &'static [LocaleMetadata] {
    GENERATED_LOCALES
}

pub fn source_locale() -> &'static LocaleMetadata {
    &GENERATED_LOCALES[GENERATED_SOURCE_LOCALE_INDEX]
}

pub fn fallback_locale() -> &'static LocaleMetadata {
    &GENERATED_LOCALES[GENERATED_FALLBACK_LOCALE_INDEX]
}

pub fn active_locale() -> &'static LocaleMetadata {
    &GENERATED_LOCALES[ACTIVE_LOCALE_INDEX.load(Ordering::Acquire)]
}

pub fn resolve_preference(preference: &str) -> &'static LocaleMetadata {
    let requested = if preference.trim().eq_ignore_ascii_case(AUTO_LANGUAGE) {
        sys_locale::get_locale()
    } else {
        Some(preference.to_string())
    };
    requested
        .as_deref()
        .and_then(resolve_locale_tag)
        .unwrap_or_else(fallback_locale)
}

pub fn canonical_preference(preference: &str) -> &'static str {
    if preference.trim().eq_ignore_ascii_case(AUTO_LANGUAGE) {
        AUTO_LANGUAGE
    } else {
        resolve_preference(preference).id
    }
}

pub fn activate_preference(preference: &str) -> &'static LocaleMetadata {
    let locale = resolve_preference(preference);
    activate_locale(locale.id).unwrap_or_else(fallback_locale)
}

/// Commits an already resolved locale to the process-wide Rust catalog.
///
/// UI adapters can first select their framework bundle and call this only
/// after that succeeds, keeping Rust-authored and UI-authored copy in sync.
pub fn activate_locale(locale_id: &str) -> Option<&'static LocaleMetadata> {
    let index = locale_index(locale_id)?;
    ACTIVE_LOCALE_INDEX.store(index, Ordering::Release);
    GENERATED_LOCALES.get(index)
}

pub fn translate(key: &str) -> Cow<'_, str> {
    translate_for(active_locale().id, key)
}

pub fn translate_for<'a>(locale_id: &str, key: &'a str) -> Cow<'a, str> {
    let index = locale_index(locale_id).unwrap_or(GENERATED_FALLBACK_LOCALE_INDEX);
    let catalog = GENERATED_CATALOGS[index];
    catalog
        .binary_search_by_key(&key, |entry| entry.key)
        .ok()
        .map(|entry_index| Cow::Borrowed(catalog[entry_index].value))
        .or_else(|| source_message(key).map(Cow::Borrowed))
        .unwrap_or_else(|| Cow::Borrowed(key))
}

/// Translates a message and substitutes named placeholders such as
/// `{version}`. Replacement values are inserted once and are never parsed as
/// templates themselves.
pub fn translate_with(key: &str, arguments: &[(&str, &str)]) -> String {
    translate_for_with(active_locale().id, key, arguments)
}

pub fn translate_for_with(locale_id: &str, key: &str, arguments: &[(&str, &str)]) -> String {
    interpolate_named(translate_for(locale_id, key).as_ref(), arguments)
}

fn interpolate_named(template: &str, arguments: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remainder = template;

    while let Some(open_index) = remainder.find('{') {
        output.push_str(&remainder[..open_index]);
        let placeholder = &remainder[open_index + 1..];
        let Some(close_index) = placeholder.find('}') else {
            output.push_str(&remainder[open_index..]);
            return output;
        };
        let name = &placeholder[..close_index];
        if let Some((_, value)) = arguments.iter().find(|(candidate, _)| *candidate == name) {
            output.push_str(value);
        } else {
            output.push_str(&remainder[open_index..open_index + close_index + 2]);
        }
        remainder = &placeholder[close_index + 1..];
    }

    output.push_str(remainder);
    output
}

pub fn source_message(key: &str) -> Option<&'static str> {
    let catalog = GENERATED_CATALOGS[GENERATED_SOURCE_LOCALE_INDEX];
    catalog
        .binary_search_by_key(&key, |entry| entry.key)
        .ok()
        .map(|entry_index| catalog[entry_index].value)
}

fn locale_index(locale_id: &str) -> Option<usize> {
    let normalized = normalize_locale(locale_id);
    GENERATED_LOCALES.iter().position(|locale| {
        normalize_locale(locale.id) == normalized
            || locale
                .aliases
                .iter()
                .any(|alias| normalize_locale(alias) == normalized)
    })
}

fn resolve_locale_tag(locale_tag: &str) -> Option<&'static LocaleMetadata> {
    let normalized = normalize_locale(locale_tag);
    if normalized.is_empty() {
        return None;
    }

    if let Some(index) = locale_index(&normalized) {
        return GENERATED_LOCALES.get(index);
    }

    // More specific system locales may safely fall back through their BCP-47
    // parents only when the manifest explicitly declares that parent. This
    // avoids arbitrarily choosing between regional variants such as pt-BR and
    // pt-PT as more languages are added.
    let mut candidate = normalized.as_str();
    while let Some(separator) = candidate.rfind('-') {
        candidate = &candidate[..separator];
        if let Some(index) = locale_index(candidate) {
            return GENERATED_LOCALES.get(index);
        }
    }
    None
}

fn normalize_locale(locale: &str) -> String {
    locale
        .trim()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_aliases_are_case_and_separator_insensitive() {
        for locale in available_locales() {
            let system_style =
                format!("{}.UTF-8", locale.id.to_ascii_uppercase().replace('-', "_"));
            assert_eq!(
                resolve_locale_tag(&system_style).map(|resolved| resolved.id),
                Some(locale.id)
            );
            for alias in locale.aliases {
                assert_eq!(
                    resolve_locale_tag(&alias.to_ascii_uppercase().replace('-', "_"))
                        .map(|resolved| resolved.id),
                    Some(locale.id)
                );
            }
        }
    }

    #[test]
    fn unknown_locale_uses_configured_fallback() {
        assert_eq!(resolve_preference("xx-Zzzz").id, fallback_locale().id);
    }

    #[test]
    fn catalogs_fall_back_to_source_for_unknown_locale() {
        assert_eq!(
            translate_for("unknown", "settings.title"),
            source_message("settings.title").expect("settings.title source message")
        );
    }

    #[test]
    fn locale_activation_is_manifest_driven() {
        for locale in available_locales() {
            assert_eq!(
                activate_locale(locale.id).map(|active| active.id),
                Some(locale.id)
            );
        }
        assert!(activate_locale("not-a-shipped-locale").is_none());
        activate_locale(fallback_locale().id).expect("restore fallback locale");
    }

    #[test]
    fn named_interpolation_supports_locale_specific_argument_order() {
        assert_eq!(
            interpolate_named(
                "Current {current}; available {available}.",
                &[("available", "2.0"), ("current", "1.0")]
            ),
            "Current 1.0; available 2.0."
        );
    }

    #[test]
    fn named_interpolation_preserves_unknown_placeholders_and_argument_braces() {
        assert_eq!(
            interpolate_named("Value: {value}; missing: {missing}", &[("value", "{raw}")]),
            "Value: {raw}; missing: {missing}"
        );
    }

    #[test]
    fn rust_only_platform_copy_remains_available_to_runtime() {
        assert_eq!(
            source_message("workflow.show_in_finder"),
            Some("Show in Finder")
        );
        assert_eq!(
            source_message("workflow.show_in_explorer"),
            Some("Show in Explorer")
        );
    }
}
