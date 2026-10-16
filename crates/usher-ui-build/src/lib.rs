//! Application-owned Cargo integration for the public Slint compiler API.
//!
//! Catalog helpers watch their original inputs. Generated files under this
//! build's `OUT_DIR` are outputs, not additional reasons to rerun the build.
//! This facade supports the four application build scripts: absolute config
//! paths, generated translation bundles, and the default Rust module layout.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub use slint_build::{CompileError, CompilerConfiguration};

const ENV_INPUTS: [&str; 8] = [
    "SLINT_STYLE",
    "SLINT_FONT_SIZES",
    "SLINT_SCALE_FACTOR",
    "SLINT_ASSET_SECTION",
    "SLINT_EMBED_RESOURCES",
    "SLINT_EMIT_DEBUG_INFO",
    "SLINT_LIVE_PREVIEW",
    "SLINT_BUNDLE_TRANSLATIONS",
];

/// Compiles a root with the same output naming and Cargo environment as Slint.
///
/// Include, library and translation paths in `config` must be absolute, as
/// required by `slint_build::compile_with_output_path`. Callers must watch the
/// original catalog/locale inputs when the translation bundle is generated.
/// Pass a root configuration without `as_library` or `rust_module`; use
/// [`compile_library_with_config`] for the application's named libraries.
/// External translation bundles require caller-owned directory watches.
pub fn compile_with_config(
    input: impl AsRef<Path>,
    config: CompilerConfiguration,
) -> Result<(), CompileError> {
    compile(input.as_ref(), config, None)
}

/// Compiles a named library in the default Rust module and emits its metadata.
/// Pass `config` without `as_library` or `rust_module`: the explicit name here
/// owns both compilation and Cargo metadata. Paths and catalog watching follow
/// [`compile_with_config`]. Custom Rust modules require a separate integration.
pub fn compile_library_with_config(
    input: impl AsRef<Path>,
    config: CompilerConfiguration,
    library_name: &str,
) -> Result<(), CompileError> {
    compile(
        input.as_ref(),
        config.as_library(library_name),
        Some(library_name),
    )
}

fn compile(
    input: &Path,
    config: CompilerConfiguration,
    library_name: Option<&str>,
) -> Result<(), CompileError> {
    let manifest = cargo_path("CARGO_MANIFEST_DIR")?;
    let out = cargo_path("OUT_DIR")?;
    // Keep compilation paths unchanged: normalizing them here would change
    // source-location metadata and invalidate otherwise equivalent generated Rust.
    let input = manifest.join(input);
    let output = out.join(
        input
            .file_stem()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("slint_out"))
            .with_extension("rs"),
    );
    if let Some(name) = library_name {
        println!("cargo::metadata=SLINT_LIBRARY_NAME={name}");
        println!(
            "cargo::metadata=SLINT_LIBRARY_PACKAGE={}",
            std::env::var("CARGO_PKG_NAME").unwrap_or_default()
        );
        println!("cargo::metadata=SLINT_LIBRARY_SOURCE={}", input.display());
    }
    // This public API retains CARGO_PKG_NAME as the translation domain.
    let dependencies = slint_build::compile_with_output_path(&input, &output, config)?;
    for dependency in
        source_dependencies(&dependencies, &manifest, &out).map_err(CompileError::SaveError)?
    {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    for name in ENV_INPUTS {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!(
        "cargo:rustc-env=SLINT_INCLUDE_GENERATED={}",
        output.display()
    );
    Ok(())
}

fn cargo_path(name: &str) -> Result<PathBuf, CompileError> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or(CompileError::NotRunViaCargo)
}

fn source_dependencies(
    dependencies: &[PathBuf],
    manifest: &Path,
    out: &Path,
) -> std::io::Result<BTreeSet<PathBuf>> {
    let out = out.canonicalize()?;
    let mut watched = BTreeSet::new();
    for dependency in dependencies {
        // Resolve dot segments and symlinks before checking containment. An
        // OUT_DIR symlink to a real external source must remain a dependency.
        // Fail on an unreadable/missing input instead of silently losing its watch.
        let original = manifest.join(dependency);
        let source = original.canonicalize()?;
        if !source.starts_with(&out) {
            // Keep the original path as well: canonical-only watches would
            // stop observing a source symlink when it is retargeted/deleted.
            watched.insert(original);
            watched.insert(source);
        } else if original.is_symlink() && !source.is_dir() {
            // An external source alias may currently point at an output but
            // later be retargeted to a real source. Watch that alias only.
            // Canonicalizing its parent excludes OUT_DIR directory aliases.
            let parent = original.parent().unwrap_or(manifest).canonicalize()?;
            if !parent.starts_with(&out) {
                watched.insert(original);
            }
        }
    }
    Ok(watched)
}

#[cfg(test)]
mod tests;
