fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Inline Slint test harnesses need element metadata for semantic queries.
    // The production MainWindow is compiled separately by usher-ui's build
    // script, so this package-local rustc environment does not affect it.
    println!("cargo:rustc-env=SLINT_EMIT_DEBUG_INFO=1");
}
