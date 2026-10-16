fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // slint::slint! accepts library mappings only as string literals. Prepare
    // the generated catalog at a stable, ignored path so integration-test
    // harnesses can import the same @i18n interface as the production UI.
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    );
    let i18n = usher_i18n_build::prepare_slint_bundle(
        &manifest_dir,
        manifest_dir.join("target/slint-macro-tests"),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare Slint translations for inline test harnesses");
    // Importing the generated I18n global alone leaves @tr on its gettext
    // fallback. Compile inline roots with the same index-identical bundle as
    // the production roots so runtime locale changes exercise real bindings.
    println!(
        "cargo:rustc-env=SLINT_BUNDLE_TRANSLATIONS={}",
        i18n.translations_path.display()
    );

    // Inline Slint test harnesses need element metadata for semantic queries.
    // The production MainWindow is compiled separately by usher-ui's build
    // script, so this package-local rustc environment does not affect it.
    println!("cargo:rustc-env=SLINT_EMIT_DEBUG_INFO=1");
}
