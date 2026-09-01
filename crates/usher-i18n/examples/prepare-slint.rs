use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let out_dir = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: prepare-slint <output-directory> <translation-domain>")?;
    let domain = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("usage: prepare-slint <output-directory> <translation-domain>")?;
    if args.next().is_some() {
        return Err("usage: prepare-slint <output-directory> <translation-domain>".into());
    }

    let prepared = usher_i18n::build_support::prepare_slint_bundle(out_dir, &domain)?;
    println!("{}", prepared.library_path.display());
    println!("{}", prepared.translations_path.display());
    Ok(())
}
