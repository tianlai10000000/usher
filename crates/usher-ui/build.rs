// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::Path;

fn main() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = slint_build::CompilerConfiguration::new().with_library_paths(HashMap::from([(
        "material".to_string(),
        repository_root.join("material-1.7.1-fixed/material.slint"),
    )]));

    slint_build::compile_with_config(repository_root.join("ui/main.slint"), config).unwrap();
}
