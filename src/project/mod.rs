use crate::{MainWindow, ProjectItem, ToastManager, ToastOptions};
use base64::Engine as _;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Global, ModelRc, VecModel};
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectConfig {
    project_name: String,
    project_alias: String,
    local_repo: String,
    remote_repo: String,
    #[serde(default)]
    logo: String,
    #[serde(default)]
    project_tags: Vec<String>,
}

pub fn setup_project(ui: &MainWindow) {
    let projects = load_projects();
    let state = Arc::new(Mutex::new(projects));
    let pending_remote_logos = Arc::new(Mutex::new(HashSet::<String>::new()));

    refresh_project_items(
        ui,
        &state,
        &pending_remote_logos,
        &ui.get_project_search_query(),
    );
    ui.set_project_logo_preview_image(resolve_project_logo_image(""));
    ui.set_project_logo_preview_status("Using app default logo".into());

    let ui_weak = ui.as_weak();
    let pending_preview_logos = pending_remote_logos.clone();
    ui.on_project_preview_logo(move |logo| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        refresh_logo_preview(&ui, &pending_preview_logos, logo.as_str());
    });

    let ui_weak = ui.as_weak();
    let state_add = state.clone();
    let pending_remote_logos_add = pending_remote_logos.clone();
    ui.on_project_add_project(
        move |name, alias, local_repo, remote_repo, logo, project_tags| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            log::info!(
                "[Project] Add project requested: name='{}', alias='{}'",
                name,
                alias
            );

            let name = name.trim();
            let alias = alias.trim();
            let local_repo = local_repo.trim();
            let remote_repo = remote_repo.trim();
            let logo = normalize_project_logo(logo.as_str());
            let project_tags = parse_project_tags(project_tags.as_str());

            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, "Please fill in all fields.");
                return;
            }

            let mut guard = match state_add.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    show_error(&ui, "Failed to update project list.");
                    return;
                }
            };

            if guard.iter().any(|item| item.project_alias == alias) {
                show_error(&ui, "Project alias already exists.");
                return;
            }

            guard.push(ProjectConfig {
                project_name: name.to_string(),
                project_alias: alias.to_string(),
                local_repo: local_repo.to_string(),
                remote_repo: remote_repo.to_string(),
                logo,
                project_tags,
            });

            if let Err(err) = write_projects(&guard) {
                show_error(&ui, &format!("Failed to save project: {err}"));
                guard.pop();
                return;
            }

            let query = ui.get_project_search_query();
            let (ui_items, remote_urls) = prepare_project_items(&guard, &query);
            ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
            prefetch_remote_logos(
                ui.as_weak(),
                state_add.clone(),
                pending_remote_logos_add.clone(),
                remote_urls,
            );
            show_success(&ui, "Project added.");
        },
    );

    let ui_weak = ui.as_weak();
    let state_update = state.clone();
    let pending_remote_logos_update = pending_remote_logos.clone();
    ui.on_project_update_project(
        move |original_alias, name, alias, local_repo, remote_repo, logo, project_tags| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            log::info!(
                "[Project] Update project requested: original_alias='{}', alias='{}'",
                original_alias,
                alias
            );

            let original_alias = original_alias.trim();
            let name = name.trim();
            let alias = alias.trim();
            let local_repo = local_repo.trim();
            let remote_repo = remote_repo.trim();
            let logo = normalize_project_logo(logo.as_str());
            let project_tags = parse_project_tags(project_tags.as_str());

            if original_alias.is_empty() {
                show_error(&ui, "Missing original project alias.");
                return;
            }
            if name.is_empty()
                || alias.is_empty()
                || local_repo.is_empty()
                || remote_repo.is_empty()
            {
                show_error(&ui, "Please fill in all fields.");
                return;
            }

            let mut guard = match state_update.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    show_error(&ui, "Failed to update project list.");
                    return;
                }
            };

            let Some(index) = guard
                .iter()
                .position(|item| item.project_alias == original_alias)
            else {
                show_error(&ui, "Project not found.");
                return;
            };

            if alias != original_alias && guard.iter().any(|item| item.project_alias == alias) {
                show_error(&ui, "Project alias already exists.");
                return;
            }

            guard[index] = ProjectConfig {
                project_name: name.to_string(),
                project_alias: alias.to_string(),
                local_repo: local_repo.to_string(),
                remote_repo: remote_repo.to_string(),
                logo,
                project_tags,
            };

            if let Err(err) = write_projects(&guard) {
                show_error(&ui, &format!("Failed to save project: {err}"));
                return;
            }

            let query = ui.get_project_search_query();
            let (ui_items, remote_urls) = prepare_project_items(&guard, &query);
            ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
            prefetch_remote_logos(
                ui.as_weak(),
                state_update.clone(),
                pending_remote_logos_update.clone(),
                remote_urls,
            );
            show_success(&ui, "Project updated.");
        },
    );

    let ui_weak = ui.as_weak();
    let state_delete = state.clone();
    let pending_remote_logos_delete = pending_remote_logos.clone();
    ui.on_project_delete_project(move |alias| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Delete project requested: alias='{}'", alias);

        let alias = alias.trim();
        if alias.is_empty() {
            show_error(&ui, "Missing project alias.");
            return;
        }

        let mut guard = match state_delete.lock() {
            Ok(guard) => guard,
            Err(_) => {
                show_error(&ui, "Failed to update project list.");
                return;
            }
        };

        let Some(index) = guard.iter().position(|item| item.project_alias == alias) else {
            show_error(&ui, "Project not found.");
            return;
        };

        guard.remove(index);
        if let Err(err) = write_projects(&guard) {
            show_error(&ui, &format!("Failed to save project: {err}"));
            return;
        }

        let query = ui.get_project_search_query();
        let (ui_items, remote_urls) = prepare_project_items(&guard, &query);
        ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
        prefetch_remote_logos(
            ui.as_weak(),
            state_delete.clone(),
            pending_remote_logos_delete.clone(),
            remote_urls,
        );
        show_success(&ui, "Project deleted.");
    });

    let ui_weak = ui.as_weak();
    let state_search = state.clone();
    let pending_remote_logos_search = pending_remote_logos.clone();
    ui.on_project_search_projects(move |query| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        let Ok(guard) = state_search.lock() else {
            show_error(&ui, "Failed to search projects.");
            return;
        };

        let (ui_items, remote_urls) = prepare_project_items(&guard, &query);
        ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
        prefetch_remote_logos(
            ui.as_weak(),
            state_search.clone(),
            pending_remote_logos_search.clone(),
            remote_urls,
        );
    });

    let ui_weak = ui.as_weak();
    let state_reload = state.clone();
    let pending_remote_logos_reload = pending_remote_logos.clone();
    ui.on_project_reload_projects(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        log::info!("[Project] Reload projects requested.");
        let list = load_projects();

        let mut guard = match state_reload.lock() {
            Ok(guard) => guard,
            Err(_) => {
                show_error(&ui, "Failed to reload projects.");
                return;
            }
        };

        *guard = list;
        let query = ui.get_project_search_query();
        let (ui_items, remote_urls) = prepare_project_items(&guard, &query);
        ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
        prefetch_remote_logos(
            ui.as_weak(),
            state_reload.clone(),
            pending_remote_logos_reload.clone(),
            remote_urls,
        );
        show_success(&ui, "Projects reloaded.");
    });
}

fn load_projects() -> Vec<ProjectConfig> {
    let Some(path) = projects_file_path() else {
        return Vec::new();
    };

    if !path.exists() {
        return Vec::new();
    }

    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Vec<ProjectConfig>>(&raw) {
            Ok(mut list) => {
                for item in &mut list {
                    item.logo = normalize_project_logo(item.logo.as_str());
                    item.project_tags = normalize_project_tags(&item.project_tags);
                }
                list
            }
            Err(err) => {
                log::error!("[Project] Failed to parse projects file: {}", err);
                Vec::new()
            }
        },
        Err(err) => {
            log::error!("[Project] Failed to read projects file: {}", err);
            Vec::new()
        }
    }
}

fn projects_file_path() -> Option<PathBuf> {
    let proj_dirs = ProjectDirs::from("com", "usher", "app")?;
    let base_dir = proj_dirs.data_local_dir();
    let project_dir = base_dir.join("project");
    Some(project_dir.join("projects.json"))
}

fn write_projects(projects: &[ProjectConfig]) -> std::io::Result<()> {
    let Some(path) = projects_file_path() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_string_pretty(projects).unwrap_or_else(|_| "[]".to_string());
    fs::write(path, data)
}

fn refresh_project_items(
    ui: &MainWindow,
    state: &Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: &Arc<Mutex<HashSet<String>>>,
    query: &str,
) {
    let Ok(guard) = state.lock() else {
        return;
    };
    let (ui_items, remote_urls) = prepare_project_items(&guard, query);
    drop(guard);
    ui.set_project_items(ModelRc::new(VecModel::from(ui_items)));
    prefetch_remote_logos(
        ui.as_weak(),
        state.clone(),
        pending_remote_logos.clone(),
        remote_urls,
    );
}

fn prepare_project_items(items: &[ProjectConfig], query: &str) -> (Vec<ProjectItem>, Vec<String>) {
    (
        to_ui_items_filtered(items, query),
        collect_remote_logo_urls(items, query),
    )
}

fn to_ui_items_filtered(items: &[ProjectConfig], query: &str) -> Vec<ProjectItem> {
    let needle = query.trim().to_lowercase();
    items
        .iter()
        .filter(|item| matches_query(item, &needle))
        .cloned()
        .map(|item| {
            let logo = item.logo;
            ProjectItem {
                project_name: item.project_name.into(),
                project_alias: item.project_alias.into(),
                local_repo: item.local_repo.into(),
                remote_repo: item.remote_repo.into(),
                logo: logo.clone().into(),
                logo_image: resolve_project_logo_image(logo.as_str()),
                project_tags_text: item.project_tags.join(", ").into(),
                project_tags: ModelRc::new(VecModel::from(
                    item.project_tags
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<_>>(),
                )),
            }
        })
        .collect()
}

fn collect_remote_logo_urls(items: &[ProjectConfig], query: &str) -> Vec<String> {
    let needle = query.trim().to_lowercase();
    items
        .iter()
        .filter(|item| matches_query(item, &needle))
        .filter_map(|item| {
            if is_valid_logo_remote_url(item.logo.as_str()) {
                Some(item.logo.clone())
            } else {
                None
            }
        })
        .collect()
}

fn matches_query(item: &ProjectConfig, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let name = item.project_name.to_lowercase();
    let alias = item.project_alias.to_lowercase();
    name.contains(needle) || alias.contains(needle)
}

pub(crate) fn normalize_project_logo(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if is_valid_logo_path(trimmed)
        || is_valid_logo_remote_url(trimmed)
        || is_valid_logo_base64(trimmed)
    {
        return trimmed.to_string();
    }

    default_logo_value()
}

fn parse_project_tags(value: &str) -> Vec<String> {
    let raw_tags = value
        .split([',', '，', ';', '；'])
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(normalize_project_tag)
        .collect::<Vec<_>>();
    normalize_project_tags(&raw_tags)
}

fn normalize_project_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let normalized_tag = normalize_project_tag(tag);
        if normalized_tag.is_empty() {
            continue;
        }
        let lower = normalized_tag.to_lowercase();
        if seen.insert(lower) {
            normalized.push(normalized_tag);
        }
    }
    normalized
}

fn normalize_project_tag(value: impl AsRef<str>) -> String {
    let trimmed = value.as_ref().trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "" => String::new(),
        "h5" => "H5".to_string(),
        "web" => "Web".to_string(),
        "rn" | "react native" | "react-native" => "RN".to_string(),
        other => {
            let mut chars = other.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_ascii_uppercase().to_string() + chars.as_str()
        }
    }
}

fn is_valid_logo_path(value: &str) -> bool {
    Path::new(value).exists()
}

fn is_valid_logo_remote_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .map(|url| {
            let scheme = url.scheme();
            scheme == "http" || scheme == "https"
        })
        .unwrap_or(false)
}

fn is_valid_logo_base64(value: &str) -> bool {
    let payload = if let Some((prefix, encoded)) = value.split_once(",") {
        if !prefix.to_ascii_lowercase().contains(";base64") {
            return false;
        }
        encoded
    } else {
        value
    };
    let payload = sanitize_base64_payload(payload);

    base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .is_ok()
        || base64::engine::general_purpose::URL_SAFE
            .decode(payload.as_bytes())
            .is_ok()
}

fn default_logo_value() -> String {
    "logo.png".to_string()
}

pub(crate) fn resolve_project_logo_image(value: &str) -> slint::Image {
    let normalized = normalize_project_logo(value);
    if normalized.is_empty() {
        return load_default_logo_image();
    }

    if is_valid_logo_path(&normalized) {
        return slint::Image::load_from_path(Path::new(&normalized))
            .unwrap_or_else(|_| load_default_logo_image());
    }

    if is_valid_logo_remote_url(&normalized) {
        if let Some(image) = load_cached_remote_logo_image(&normalized) {
            return image;
        }
        return load_default_logo_image();
    }

    if is_valid_logo_base64(&normalized) {
        if let Some(image) = load_base64_logo_image(&normalized) {
            return image;
        }
        return load_default_logo_image();
    }

    load_default_logo_image()
}

fn load_default_logo_image() -> slint::Image {
    slint::Image::load_from_path(Path::new("logo.png")).unwrap_or_else(|_| slint::Image::default())
}

fn load_cached_remote_logo_image(url: &str) -> Option<slint::Image> {
    let path = find_cached_remote_logo_path(url)?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let bytes = fs::read(&path).ok()?;
    load_image_from_bytes(bytes.as_slice(), ext.as_str())
}

fn load_base64_logo_image(value: &str) -> Option<slint::Image> {
    let (payload, ext) = if let Some((prefix, encoded)) = value.split_once(',') {
        let lower = prefix.to_ascii_lowercase();
        if !lower.contains(";base64") {
            return None;
        }
        let ext = infer_ext_from_data_prefix(prefix).unwrap_or("png");
        (encoded, ext)
    } else {
        (value, "png")
    };
    let payload = sanitize_base64_payload(payload);

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload.as_bytes()))
        .ok()?;
    load_image_from_bytes(decoded.as_slice(), ext)
}

fn load_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    if ext == "svg" {
        return load_svg_image_from_bytes(bytes, ext);
    }

    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        decoded.as_raw().as_slice(),
        width,
        height,
    );
    Some(slint::Image::from_rgba8(buffer))
}

fn load_svg_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    let cache_path = byte_cache_path(bytes, ext)?;
    slint::Image::load_from_path(&cache_path).ok()
}

fn sanitize_base64_payload(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

fn byte_cache_path(bytes: &[u8], ext: &str) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    ext.hash(&mut hasher);
    let hash = hasher.finish();

    let cache_dir = logo_cache_dir()?;
    let cache_path = cache_dir.join(format!("{hash:016x}.{ext}"));
    if !cache_path.exists() {
        fs::write(&cache_path, bytes).ok()?;
    }
    Some(cache_path)
}

fn logo_cache_dir() -> Option<PathBuf> {
    let cache_dir = std::env::temp_dir().join("usher-logo-cache");
    fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

fn remote_logo_cache_base(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn remote_logo_cache_path(url: &str, ext: &str) -> Option<PathBuf> {
    Some(logo_cache_dir()?.join(format!("remote-{}.{}", remote_logo_cache_base(url), ext)))
}

fn find_cached_remote_logo_path(url: &str) -> Option<PathBuf> {
    for ext in ["png", "jpg", "webp", "gif", "svg"] {
        let path = remote_logo_cache_path(url, ext)?;
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn prefetch_remote_logos(
    ui_weak: slint::Weak<MainWindow>,
    state: Arc<Mutex<Vec<ProjectConfig>>>,
    pending_remote_logos: Arc<Mutex<HashSet<String>>>,
    remote_urls: Vec<String>,
) {
    for url in remote_urls {
        if find_cached_remote_logo_path(url.as_str()).is_some() {
            continue;
        }

        let mut should_spawn = false;
        if let Ok(mut guard) = pending_remote_logos.lock() {
            should_spawn = guard.insert(url.clone());
        }
        if !should_spawn {
            continue;
        }

        let ui_weak_task = ui_weak.clone();
        let state_task = state.clone();
        let pending_task = pending_remote_logos.clone();
        std::thread::spawn(move || {
            let downloaded = download_remote_logo_to_cache(url.as_str()).is_some();
            if let Ok(mut guard) = pending_task.lock() {
                guard.remove(url.as_str());
            }
            if !downloaded {
                return;
            }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak_task.upgrade() else {
                    return;
                };
                let query = ui.get_project_search_query();
                refresh_project_items(&ui, &state_task, &pending_task, &query);
            });
        });
    }
}

fn download_remote_logo_to_cache(url: &str) -> Option<PathBuf> {
    let response = reqwest::blocking::get(url).ok()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().ok()?;
    let ext = infer_ext_from_content_type(content_type.as_str())
        .or_else(|| infer_ext_from_url(url))
        .unwrap_or("png");
    let cache_path = remote_logo_cache_path(url, ext)?;
    if !cache_path.exists() {
        fs::write(&cache_path, bytes.as_ref()).ok()?;
    }
    Some(cache_path)
}

fn refresh_logo_preview(
    ui: &MainWindow,
    pending_remote_logos: &Arc<Mutex<HashSet<String>>>,
    logo: &str,
) {
    let trimmed = logo.trim();
    let normalized = normalize_project_logo(logo);
    ui.set_project_logo_preview_status(preview_logo_status(trimmed).into());
    ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));

    if !is_valid_logo_remote_url(&normalized) || find_cached_remote_logo_path(&normalized).is_some()
    {
        return;
    }

    let mut should_spawn = false;
    if let Ok(mut guard) = pending_remote_logos.lock() {
        should_spawn = guard.insert(normalized.clone());
    }
    if !should_spawn {
        return;
    }

    let ui_weak = ui.as_weak();
    let pending_task = pending_remote_logos.clone();
    std::thread::spawn(move || {
        let downloaded = download_remote_logo_to_cache(normalized.as_str()).is_some();
        if let Ok(mut guard) = pending_task.lock() {
            guard.remove(normalized.as_str());
        }
        if !downloaded {
            return;
        }

        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_project_new_logo().as_str() == normalized {
                ui.set_project_logo_preview_status("Remote URL".into());
                ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));
            }
        });
    });
}

fn preview_logo_status(value: &str) -> &'static str {
    if value.is_empty() {
        "Using app default logo"
    } else if is_valid_logo_path(value) {
        "Local file"
    } else if is_valid_logo_base64(value) {
        "Base64 image"
    } else if is_valid_logo_remote_url(value) {
        if find_cached_remote_logo_path(value).is_some() {
            "Remote URL"
        } else {
            "Remote URL, loading preview..."
        }
    } else {
        "Invalid input, using default logo"
    }
}

fn infer_ext_from_content_type(content_type: &str) -> Option<&'static str> {
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("image/png") {
        Some("png")
    } else if lower.contains("image/jpeg") || lower.contains("image/jpg") {
        Some("jpg")
    } else if lower.contains("image/webp") {
        Some("webp")
    } else if lower.contains("image/gif") {
        Some("gif")
    } else if lower.contains("image/svg") {
        Some("svg")
    } else {
        None
    }
}

fn infer_ext_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        Some("png")
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        Some("jpg")
    } else if lower.contains(".webp") {
        Some("webp")
    } else if lower.contains(".gif") {
        Some("gif")
    } else if lower.contains(".svg") {
        Some("svg")
    } else {
        None
    }
}

fn infer_ext_from_data_prefix(prefix: &str) -> Option<&'static str> {
    let lower = prefix.to_ascii_lowercase();
    if lower.starts_with("data:image/png") {
        Some("png")
    } else if lower.starts_with("data:image/jpeg") || lower.starts_with("data:image/jpg") {
        Some("jpg")
    } else if lower.starts_with("data:image/webp") {
        Some("webp")
    } else if lower.starts_with("data:image/gif") {
        Some("gif")
    } else if lower.starts_with("data:image/svg") {
        Some("svg")
    } else {
        None
    }
}

fn show_success(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_success(ToastOptions {
        message: message.into(),
        duration: 2200,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}

fn show_error(ui: &MainWindow, message: &str) {
    let manager = ToastManager::get(ui);
    manager.invoke_error(ToastOptions {
        message: message.into(),
        duration: 2600,
        position: "middle".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: false,
        loading_type: "".into(),
        width: "auto".into(),
    });
}
