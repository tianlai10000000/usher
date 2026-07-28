//! Local changed-file tree construction and filter classification.

use std::borrow::Borrow;
use std::collections::{BTreeMap, HashSet};

use crate::WorkflowPrTreeItem;
use crate::git::PullRequestDiffFile;

#[derive(Default)]
struct FileTreeNode {
    directories: BTreeMap<String, FileTreeNode>,
    files: Vec<usize>,
}

pub(super) fn build_tree_items<T>(
    files: &[T],
    query: &str,
    collapsed: &HashSet<String>,
    selected_extensions: &HashSet<String>,
    only_manifest_files: bool,
    show_viewed_files: bool,
) -> Vec<WorkflowPrTreeItem>
where
    T: Borrow<PullRequestDiffFile>,
{
    let normalized_query = query.trim().to_ascii_lowercase();
    let mut root = FileTreeNode::default();
    for (index, file) in files.iter().enumerate() {
        let file = file.borrow();
        if !selected_extensions.contains(file_extension(file.path.as_str()).as_str())
            || (only_manifest_files && !is_manifest_file(file.path.as_str()))
            || (!show_viewed_files && file.viewed)
        {
            continue;
        }
        if !normalized_query.is_empty()
            && !file
                .path
                .to_ascii_lowercase()
                .contains(normalized_query.as_str())
        {
            continue;
        }
        let mut segments = file
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        segments.pop();
        let mut node = &mut root;
        for segment in segments {
            node = node.directories.entry(segment.to_string()).or_default();
        }
        node.files.push(index);
    }

    let mut items = Vec::new();
    flatten_tree(
        &root,
        "",
        0,
        files,
        collapsed,
        !normalized_query.is_empty(),
        &mut items,
    );
    items
}

pub(super) fn extension_counts<T>(files: &[T]) -> BTreeMap<String, usize>
where
    T: Borrow<PullRequestDiffFile>,
{
    let mut counts = BTreeMap::new();
    for file in files {
        let file = file.borrow();
        *counts
            .entry(file_extension(file.path.as_str()))
            .or_insert(0) += 1;
    }
    counts
}

fn file_extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some((stem, extension)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !extension.is_empty()
    {
        return format!(".{}", extension.to_ascii_lowercase());
    }
    "No extension".to_string()
}

fn is_manifest_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "gemfile"
            | "gemfile.lock"
            | "go.mod"
            | "go.sum"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "gradle.lockfile"
            | "requirements.txt"
            | "pipfile"
            | "pipfile.lock"
            | "poetry.lock"
            | "pyproject.toml"
            | "composer.json"
            | "composer.lock"
            | "pubspec.yaml"
            | "pubspec.lock"
            | "podfile"
            | "podfile.lock"
            | "packages.config"
            | "mix.exs"
            | "mix.lock"
            | "package.swift"
            | "package.resolved"
            | "vcpkg.json"
            | "conanfile.py"
    ) || lower.ends_with(".csproj")
        || lower.ends_with(".fsproj")
        || lower.ends_with(".vbproj")
}

fn flatten_tree<T>(
    node: &FileTreeNode,
    parent_path: &str,
    depth: i32,
    files: &[T],
    collapsed: &HashSet<String>,
    force_expanded: bool,
    items: &mut Vec<WorkflowPrTreeItem>,
) where
    T: Borrow<PullRequestDiffFile>,
{
    for (name, child) in &node.directories {
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}/{name}")
        };
        let expanded = force_expanded || !collapsed.contains(path.as_str());
        items.push(WorkflowPrTreeItem {
            kind: "folder".into(),
            name: name.clone().into(),
            path: path.clone().into(),
            depth,
            file_index: -1,
            expanded,
            added_lines: 0,
            removed_lines: 0,
            change_type: "".into(),
            file_count: count_files(child) as i32,
        });
        if expanded {
            flatten_tree(
                child,
                path.as_str(),
                depth + 1,
                files,
                collapsed,
                force_expanded,
                items,
            );
        }
    }

    let mut file_indexes = node.files.clone();
    file_indexes
        .sort_by(|left, right| files[*left].borrow().path.cmp(&files[*right].borrow().path));
    for index in file_indexes {
        let file = files[index].borrow();
        let name = file.path.rsplit('/').next().unwrap_or(file.path.as_str());
        items.push(WorkflowPrTreeItem {
            kind: "file".into(),
            name: name.into(),
            path: file.path.clone().into(),
            depth,
            file_index: index as i32,
            expanded: false,
            added_lines: file.added_lines as i32,
            removed_lines: file.removed_lines as i32,
            change_type: file.change_type.clone().into(),
            file_count: 0,
        });
    }
}

fn count_files(node: &FileTreeNode) -> usize {
    node.files.len() + node.directories.values().map(count_files).sum::<usize>()
}
