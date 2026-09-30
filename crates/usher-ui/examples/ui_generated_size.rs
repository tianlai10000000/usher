use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_ROOTS: &[&str] = &[
    "ui/main.slint",
    "ui/views/workflow/pr_review_workspace.slint",
    "ui/views/workflow/workflow.slint",
    "ui/views/project.slint",
    "ui/views/chat.slint",
    "ui/views/settings.slint",
];

fn main() -> Result<(), Box<dyn Error>> {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let requested_roots = std::env::args().skip(1).collect::<Vec<_>>();
    let roots = if requested_roots.is_empty() {
        DEFAULT_ROOTS.iter().map(ToString::to_string).collect()
    } else {
        requested_roots
    };

    println!("root\tbytes\tlines\titem_trees\tdependencies\tcompile_ms");
    for (index, root) in roots.iter().enumerate() {
        measure_root(&repository_root, root, index)?;
    }

    Ok(())
}

fn measure_root(
    repository_root: &Path,
    requested_root: &str,
    index: usize,
) -> Result<(), Box<dyn Error>> {
    let input = absolute_input(repository_root, requested_root).canonicalize()?;
    let output = std::env::temp_dir().join(format!(
        "usher-ui-generated-size-{}-{index}.rs",
        std::process::id()
    ));
    let ui_components_library = if cfg!(feature = "ui-components") {
        repository_root.join("ui/features/ui_components_enabled.slint")
    } else {
        repository_root.join("ui/features/ui_components_disabled.slint")
    };
    let pr_review_library = if cfg!(feature = "dev-shell") {
        repository_root.join("ui/features/pr_review/dev_shell.slint")
    } else {
        repository_root.join("ui/features/pr_review/full.slint")
    };
    let ai_review_library = if cfg!(feature = "ai-code-review") {
        repository_root.join("ui/features/ai_review/full.slint")
    } else {
        repository_root.join("ui/features/ai_review/disabled.slint")
    };
    let config = slint_build::CompilerConfiguration::new().with_library_paths(HashMap::from([
        (
            "material".to_string(),
            repository_root.join("material-1.18.1/material.slint"),
        ),
        ("ui-components".to_string(), ui_components_library),
        ("pr-review".to_string(), pr_review_library),
        ("ai-review".to_string(), ai_review_library),
    ]));

    let started = Instant::now();
    let dependencies = slint_build::compile_with_output_path(&input, &output, config)?;
    let elapsed = started.elapsed();
    let generated = std::fs::read_to_string(&output)?;
    if std::env::var_os("USHER_UI_SIZE_KEEP_OUTPUT").is_some() {
        eprintln!("kept generated Rust at {}", output.display());
    } else {
        std::fs::remove_file(&output)?;
    }
    let dependency_count = dependencies
        .into_iter()
        .collect::<BTreeSet<PathBuf>>()
        .len();
    let display_root = input.strip_prefix(repository_root).unwrap_or(&input);

    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        display_root.display(),
        generated.len(),
        generated.lines().count(),
        generated.matches("fn item_tree").count(),
        dependency_count,
        elapsed.as_millis()
    );

    Ok(())
}

fn absolute_input(repository_root: &Path, requested_root: &str) -> PathBuf {
    let requested_root = Path::new(requested_root);
    if requested_root.is_absolute() {
        requested_root.to_path_buf()
    } else {
        repository_root.join(requested_root)
    }
}
