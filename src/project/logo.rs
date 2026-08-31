//! Project logo ownership boundary.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use slint::ComponentHandle;

use crate::MainWindow;
use crate::http::{self, RequestOptions};

use super::state::ProjectStateFacade;

const PROJECT_LOGO_RASTER_MAX_SIDE: u32 = 128;

thread_local! {
    static PROJECT_LOGO_IMAGE_CACHE: RefCell<HashMap<String, slint::Image>> =
        RefCell::new(HashMap::new());
}

pub(super) fn normalize_project_logo(value: &str) -> String {
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

pub(super) fn is_valid_logo_path(value: &str) -> bool {
    Path::new(value).exists()
}

pub(super) fn is_valid_logo_remote_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .map(|url| {
            let scheme = url.scheme();
            scheme == "http" || scheme == "https"
        })
        .unwrap_or(false)
}

pub(super) fn is_valid_logo_base64(value: &str) -> bool {
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

pub(super) fn default_logo_value() -> String {
    "logo.png".to_string()
}

pub(crate) fn resolve_project_logo_image(value: &str) -> slint::Image {
    let normalized = normalize_project_logo(value);
    let cache_key = logo_image_cache_key(&normalized);

    if is_valid_logo_remote_url(&normalized) {
        return resolve_remote_project_logo_image(&normalized, cache_key);
    }

    if let Some(image) = cached_logo_image(&cache_key) {
        return image;
    }

    let image = if normalized.is_empty() {
        load_default_logo_image()
    } else if is_valid_logo_path(&normalized) {
        load_project_logo_path(Path::new(&normalized)).unwrap_or_else(load_default_logo_image)
    } else if is_valid_logo_base64(&normalized) {
        if let Some(image) = load_base64_logo_image(&normalized) {
            image
        } else {
            load_default_logo_image()
        }
    } else {
        load_default_logo_image()
    };

    cache_logo_image(cache_key, image.clone());
    image
}

/// Remote logos briefly fall back to the app default while the file downloads.
/// That fallback must not be stored under the remote URL key, or a later
/// successful download keeps showing the default for the rest of the session.
pub(super) fn resolve_remote_project_logo_image(url: &str, cache_key: String) -> slint::Image {
    if find_cached_remote_logo_path(url).is_some() {
        if let Some(image) = cached_logo_image(&cache_key) {
            return image;
        }
        if let Some(image) = load_cached_remote_logo_image(url) {
            cache_logo_image(cache_key, image.clone());
            return image;
        }
        // Disk entry exists but could not be decoded; avoid caching the fallback.
        return load_default_logo_image();
    }

    invalidate_cached_logo_image(&cache_key);
    load_default_logo_image()
}

pub(super) fn cached_logo_image(key: &str) -> Option<slint::Image> {
    PROJECT_LOGO_IMAGE_CACHE.with(|cache| cache.borrow().get(key).cloned())
}

pub(super) fn cache_logo_image(key: String, image: slint::Image) {
    PROJECT_LOGO_IMAGE_CACHE.with(|cache| {
        cache.borrow_mut().insert(key, image);
    });
}

pub(super) fn invalidate_cached_logo_image(key: &str) {
    PROJECT_LOGO_IMAGE_CACHE.with(|cache| {
        cache.borrow_mut().remove(key);
    });
}

pub(super) fn logo_image_cache_key(normalized: &str) -> String {
    if normalized.is_empty() || normalized == default_logo_value() {
        return "default".to_string();
    }

    if is_valid_logo_path(normalized) {
        return format!("path:{normalized}");
    }

    if is_valid_logo_remote_url(normalized) {
        return format!("remote:{normalized}");
    }

    if is_valid_logo_base64(normalized) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        normalized.hash(&mut hasher);
        return format!("base64:{:016x}", hasher.finish());
    }

    format!("fallback:{normalized}")
}

pub(super) fn load_default_logo_image() -> slint::Image {
    load_image_from_bytes(include_bytes!("../../logo.png"), "png").unwrap_or_default()
}

pub(super) fn load_project_logo_path(path: &Path) -> Option<slint::Image> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "svg" {
        return slint::Image::load_from_path(path).ok();
    }
    let bytes = fs::read(path).ok()?;
    load_image_from_bytes(bytes.as_slice(), ext.as_str())
}

pub(super) fn load_cached_remote_logo_image(url: &str) -> Option<slint::Image> {
    let path = find_cached_remote_logo_path(url)?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let bytes = fs::read(&path).ok()?;
    load_image_from_bytes(bytes.as_slice(), ext.as_str())
}

pub(super) fn load_base64_logo_image(value: &str) -> Option<slint::Image> {
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

pub(super) fn load_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    if ext == "svg" {
        return load_svg_image_from_bytes(bytes, ext);
    }

    let decoded = image::load_from_memory(bytes).ok()?;
    let decoded = if decoded.width() > PROJECT_LOGO_RASTER_MAX_SIDE
        || decoded.height() > PROJECT_LOGO_RASTER_MAX_SIDE
    {
        decoded.thumbnail(PROJECT_LOGO_RASTER_MAX_SIDE, PROJECT_LOGO_RASTER_MAX_SIDE)
    } else {
        decoded
    }
    .to_rgba8();
    let (width, height) = decoded.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        decoded.as_raw().as_slice(),
        width,
        height,
    );
    Some(slint::Image::from_rgba8(buffer))
}

pub(super) fn load_svg_image_from_bytes(bytes: &[u8], ext: &str) -> Option<slint::Image> {
    let cache_path = byte_cache_path(bytes, ext)?;
    slint::Image::load_from_path(&cache_path).ok()
}

pub(super) fn sanitize_base64_payload(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

pub(super) fn byte_cache_path(bytes: &[u8], ext: &str) -> Option<PathBuf> {
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

pub(super) fn logo_cache_dir() -> Option<PathBuf> {
    let cache_dir = std::env::temp_dir().join("usher-logo-cache");
    fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

pub(super) fn remote_logo_cache_base(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn remote_logo_cache_path(url: &str, ext: &str) -> Option<PathBuf> {
    Some(logo_cache_dir()?.join(format!("remote-{}.{}", remote_logo_cache_base(url), ext)))
}

pub(super) fn find_cached_remote_logo_path(url: &str) -> Option<PathBuf> {
    for ext in ["png", "jpg", "webp", "gif", "svg"] {
        let path = remote_logo_cache_path(url, ext)?;
        if path.exists() {
            return Some(path);
        }
    }
    None
}

pub(super) async fn download_remote_logo_to_cache(url: &str) -> Option<PathBuf> {
    let response = http::send(
        http::get(url),
        RequestOptions::new("Download project logo").no_retry(),
    )
    .await
    .ok()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().await.ok()?;
    let ext = infer_ext_from_content_type(content_type.as_str())
        .or_else(|| infer_ext_from_url(url))
        .unwrap_or("png");
    let cache_path = remote_logo_cache_path(url, ext)?;
    if !cache_path.exists() {
        tokio::fs::write(&cache_path, bytes.as_slice()).await.ok()?;
    }
    Some(cache_path)
}

pub(super) fn refresh_logo_preview(ui: &MainWindow, state: &ProjectStateFacade, logo: &str) {
    let trimmed = logo.trim();
    let normalized = normalize_project_logo(logo);
    ui.set_project_logo_preview_status(preview_logo_status(trimmed).into());
    if is_valid_logo_remote_url(&normalized) && find_cached_remote_logo_path(&normalized).is_some()
    {
        // Re-read from disk so a previously cached pending-default cannot stick.
        invalidate_cached_logo_image(&logo_image_cache_key(&normalized));
    }
    ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));

    if !is_valid_logo_remote_url(&normalized) || find_cached_remote_logo_path(&normalized).is_some()
    {
        return;
    }

    if !state.begin_remote_logo(normalized.clone()) {
        return;
    }

    let ui_weak = ui.as_weak();
    let state = state.clone();
    crate::app_runtime::shared_runtime().spawn(async move {
        let downloaded = download_remote_logo_to_cache(normalized.as_str())
            .await
            .is_some();
        state.finish_remote_logo(normalized.clone());
        if !downloaded {
            return;
        }

        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_project_new_logo().as_str() == normalized {
                invalidate_cached_logo_image(&logo_image_cache_key(&normalized));
                ui.set_project_logo_preview_status("Remote URL".into());
                ui.set_project_logo_preview_image(resolve_project_logo_image(&normalized));
            }
        });
    });
}

pub(super) fn preview_logo_status(value: &str) -> &'static str {
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

pub(super) fn infer_ext_from_content_type(content_type: &str) -> Option<&'static str> {
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

pub(super) fn infer_ext_from_url(url: &str) -> Option<&'static str> {
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

pub(super) fn infer_ext_from_data_prefix(prefix: &str) -> Option<&'static str> {
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
