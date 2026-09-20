fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=i18n/manifest.toml");
    println!("cargo:rerun-if-changed=i18n/messages.toml");
    println!("cargo:rerun-if-changed=i18n/locales");

    // slint::slint! accepts library mappings only as string literals. Prepare
    // the generated catalog at a stable, ignored path so integration-test
    // harnesses can import the same @i18n interface as the production UI.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    usher_i18n::build_support::prepare_slint_bundle(
        manifest_dir.join("target/slint-macro-tests"),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare Slint translations for inline test harnesses");

    // Inline Slint test harnesses need element metadata for semantic queries.
    // The production MainWindow is compiled separately by usher-ui's build
    // script, so this package-local rustc environment does not affect it.
    println!("cargo:rustc-env=SLINT_EMIT_DEBUG_INFO=1");
}
