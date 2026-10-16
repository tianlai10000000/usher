use std::fmt::Write as _;

use crate::BuildResult;
use crate::catalog::LocaleCatalog;
use crate::validation::{Manifest, MessageSpec, slint_property_name};

pub(crate) fn generate_rust(
    manifest: &Manifest,
    catalogs: &[LocaleCatalog],
) -> BuildResult<String> {
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
    Ok(output)
}

pub(crate) fn generate_slint(messages: &[MessageSpec]) -> BuildResult<String> {
    let mut slint = String::from(
        "// Generated from i18n/messages.toml. Do not edit.\n\nexport global I18n {\n",
    );
    for message in messages {
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
