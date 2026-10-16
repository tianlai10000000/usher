use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Resolve Cargo's current checkout when a shared target reuses this script.
    let repository_root = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or("Cargo did not provide CARGO_MANIFEST_DIR")?,
    )
    .join("../..");
    let out_dir =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?);
    usher_i18n_build::compile_rust_catalog(&repository_root, &out_dir)
}
