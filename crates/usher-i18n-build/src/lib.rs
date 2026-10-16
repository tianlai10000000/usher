//! Catalog-free build support. Inputs are resolved from each caller's checkout.
//!
//! Keeping this crate independent of generated runtime catalogs prevents a
//! Rust-only copy change from rebuilding the static Slint compilation units.

mod catalog;
mod codegen;
mod validation;

use std::fs;
use std::path::{Path, PathBuf};

use catalog::{Scope, load_catalogs, load_messages};
pub use validation::LocaleSpec;

pub type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;
pub const AUTO_LANGUAGE: &str = "auto";

#[derive(Debug)]
pub struct PreparedSlintBundle {
    pub library_path: PathBuf,
    pub translations_path: PathBuf,
    pub locales: Vec<LocaleSpec>,
}

/// Compiles both message scopes into the application-linked runtime catalog.
/// Coverage remains one aggregate threshold per locale, as before the split.
pub fn compile_rust_catalog(repository_root: &Path, out_dir: &Path) -> BuildResult<()> {
    let i18n_root = repository_root.join("i18n");
    let manifest = validation::load_manifest(&i18n_root)?;
    let messages = load_messages(&i18n_root, Scope::Slint)?;
    let rust_messages = load_messages(&i18n_root, Scope::Rust)?;
    let combined_messages =
        validation::validate_messages(messages.iter().chain(&rust_messages).cloned().collect())?;
    let mut catalogs = load_catalogs(&i18n_root, &manifest, &messages, Scope::Slint)?;
    let rust_catalogs = load_catalogs(&i18n_root, &manifest, &rust_messages, Scope::Rust)?;
    for (catalog, rust_catalog) in catalogs.iter_mut().zip(rust_catalogs) {
        catalog.values.extend(rust_catalog.values);
        catalog.translated += rust_catalog.translated;
    }
    validation::validate_coverage(&manifest, &catalogs, combined_messages.len())?;
    let generated = codegen::generate_rust(&manifest, &catalogs)?;
    write_if_changed(&out_dir.join("catalogs.rs"), generated.as_bytes())?;
    Ok(())
}

/// Prepares the complete Slint-visible catalog for one translation domain.
/// Rust-only registries and PO files are neither read nor watched here.
pub fn prepare_slint_bundle(
    repository_root: &Path,
    out_dir: impl AsRef<Path>,
    package_name: &str,
) -> BuildResult<PreparedSlintBundle> {
    if package_name.is_empty()
        || !package_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("invalid Slint translation domain: {package_name:?}").into());
    }
    let i18n_root = repository_root.join("i18n");
    let manifest = validation::load_manifest(&i18n_root)?;
    let messages = load_messages(&i18n_root, Scope::Slint)?;
    let catalogs = load_catalogs(&i18n_root, &manifest, &messages, Scope::Slint)?;
    let generated_root = out_dir.as_ref().join("usher-i18n");
    let library_path = generated_root.join("i18n.slint");
    write_if_changed(
        &library_path,
        codegen::generate_slint(&messages)?.as_bytes(),
    )?;
    let translations_path = generated_root.join("translations");
    for catalog in catalogs {
        if let Some(contents) = catalog.po_source {
            let domain_path = translations_path
                .join(catalog.id)
                .join("LC_MESSAGES")
                .join(format!("{package_name}.po"));
            write_if_changed(&domain_path, contents.as_bytes())?;
        }
    }
    Ok(PreparedSlintBundle {
        library_path,
        translations_path,
        locales: manifest.locales,
    })
}

fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[cfg(test)]
mod tests;
