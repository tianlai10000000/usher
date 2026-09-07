//! Worker-only decoding. UI receives shared pixels, never paths or encoded bytes.
use super::logo::{
    find_cached_remote_logo_path, is_valid_logo_remote_url, sanitize_base64_payload,
};
use base64::Engine as _;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const SIDE: u32 = 128;

#[derive(Debug)]
pub(super) struct PreparedLogo {
    pub(super) pixels: SharedPixelBuffer<Rgba8Pixel>,
    pub(super) source_bytes: u64,
    pub(super) decoded_bytes: u64,
    pub(super) status: &'static str,
}

pub(super) struct LogoCache {
    entries: HashMap<String, (u64, Arc<PreparedLogo>)>,
    default: Arc<PreparedLogo>,
    svg_options: Option<resvg::usvg::Options<'static>>,
    pub(super) decoded: usize,
}

impl LogoCache {
    pub(super) fn new() -> Self {
        let pixels = decode_raster(include_bytes!("../../logo.png")).expect("bundled logo decodes");
        Self {
            entries: HashMap::new(),
            default: Arc::new(PreparedLogo {
                pixels: pixels.0,
                source_bytes: 0,
                decoded_bytes: 0,
                status: "projects.using_default_logo",
            }),
            svg_options: None,
            decoded: 0,
        }
    }

    pub(super) fn retain(&mut self, sources: &HashSet<String>) {
        self.entries.retain(|source, _| sources.contains(source));
    }

    pub(super) fn prepare(&mut self, value: &str, version: u64) -> Arc<PreparedLogo> {
        let value = value.trim();
        if value.is_empty() || value == "logo.png" {
            return self.default.clone();
        }
        if let Some((cached_version, logo)) = self.entries.get(value) {
            if *cached_version == version {
                return logo.clone();
            }
        }
        let remote = is_valid_logo_remote_url(value);
        let (bytes, status, base_dir) = if remote {
            (
                find_cached_remote_logo_path(value).and_then(|p| read_bounded(&p)),
                "projects.logo_remote_url",
                None,
            )
        } else if Path::new(value).is_file() {
            (
                read_bounded(Path::new(value)),
                "projects.logo_local_file",
                Path::new(value).parent().map(Path::to_path_buf),
            )
        } else {
            let payload = value.split_once(',').map_or(value, |(_, payload)| payload);
            let bytes = (payload.len() as u64 <= MAX_SOURCE_BYTES * 4 / 3 + 4)
                .then(|| sanitize_base64_payload(payload))
                .and_then(|p| {
                    base64::engine::general_purpose::STANDARD
                        .decode(&p)
                        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&p))
                        .ok()
                });
            (bytes, "projects.logo_base64_image", None)
        };
        let decoded = bytes.as_ref().and_then(|bytes| {
            self.decoded += 1;
            if image::guess_format(bytes).is_ok() {
                return decode_raster(bytes);
            }
            let options = self.svg_options.get_or_insert_with(|| {
                let mut options = resvg::usvg::Options::default();
                // Load the application font only if SVG text actually needs it.
                // Scanning every installed font can retain hundreds of MiB.
                options.font_family = "Noto Sans SC".into();
                let select = resvg::usvg::FontResolver::default_font_selector();
                options.font_resolver.select_font = Box::new(move |font, database| {
                    if database.is_empty() {
                        let db = Arc::make_mut(database);
                        let bytes: &'static [u8] = include_bytes!(
                            "../../ui/assets/fonts/Noto_Sans_SC/static/NotoSansSC-Regular.ttf"
                        );
                        db.load_font_source(resvg::usvg::fontdb::Source::Binary(Arc::new(bytes)));
                        db.set_serif_family("Noto Sans SC");
                        db.set_sans_serif_family("Noto Sans SC");
                        db.set_monospace_family("Noto Sans SC");
                    }
                    select(font, database)
                });
                options
            });
            options.resources_dir = base_dir;
            let tree = resvg::usvg::Tree::from_data(bytes, options).ok()?;
            let size = tree.size();
            let scale = (SIDE as f32 / size.width()).min(SIDE as f32 / size.height());
            let width = (size.width() * scale).round().max(1.) as u32;
            let height = (size.height() * scale).round().max(1.) as u32;
            let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
            let mut pixmap =
                resvg::tiny_skia::PixmapMut::from_bytes(pixels.make_mut_bytes(), width, height)?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap,
            );
            // resvg produces premultiplied RGBA; normalize once on the worker.
            for pixel in pixels.make_mut_slice() {
                if pixel.a != 0 {
                    let a = u32::from(pixel.a);
                    pixel.r = (u32::from(pixel.r) * 255 / a).min(255) as u8;
                    pixel.g = (u32::from(pixel.g) * 255 / a).min(255) as u8;
                    pixel.b = (u32::from(pixel.b) * 255 / a).min(255) as u8;
                }
            }
            Some((pixels, u64::from(width) * u64::from(height) * 4))
        });
        let logo = match decoded {
            Some((pixels, decoded_bytes)) => Arc::new(PreparedLogo {
                pixels,
                decoded_bytes,
                source_bytes: bytes.as_ref().map_or(0, |b| b.len() as u64),
                status,
            }),
            None => Arc::new(PreparedLogo {
                pixels: self.default.pixels.clone(),
                source_bytes: 0,
                decoded_bytes: 0,
                status: if remote && bytes.is_none() {
                    "projects.logo_remote_loading"
                } else {
                    "projects.invalid_logo_default"
                },
            }),
        };
        self.entries
            .insert(value.to_owned(), (version, logo.clone()));
        logo
    }
}

fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_SOURCE_BYTES).then_some(bytes)
}

fn decode_raster(bytes: &[u8]) -> Option<(SharedPixelBuffer<Rgba8Pixel>, u64)> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    let decoded_bytes = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    if decoded_bytes > MAX_DECODED_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let decoded = if width > SIDE || height > SIDE {
        decoded.thumbnail(SIDE, SIDE)
    } else {
        decoded
    }
    .into_rgba8();
    Some((
        SharedPixelBuffer::clone_from_slice(decoded.as_raw(), decoded.width(), decoded.height()),
        decoded_bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_pixels_survive_worker_exit_and_repeated_sources_do_not_decode_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logo.png");
        image::DynamicImage::new_rgba8(512, 256)
            .save(&path)
            .unwrap();
        let value = path.to_str().unwrap().to_owned();
        let prepared = std::thread::spawn(move || {
            let mut cache = LogoCache::new();
            let first = cache.prepare(&value, 0);
            let second = cache.prepare(&value, 0);
            assert!(Arc::ptr_eq(&first, &second));
            assert_eq!(cache.decoded, 1);
            assert_eq!((first.pixels.width(), first.pixels.height()), (128, 64));
            let refreshed = cache.prepare(&value, 1);
            assert!(!Arc::ptr_eq(&first, &refreshed));
            cache.retain(&HashSet::new());
            assert!(cache.entries.is_empty());
            first
        })
        .join()
        .unwrap();
        assert_eq!(prepared.decoded_bytes, 512 * 256 * 4);
    }
    #[test]
    fn svg_text_uses_shared_bundled_font_without_scanning_system_fonts() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="64"><text x="2" y="45" font-size="32">项目 A</text></svg>"#;
        let data = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        );
        let mut cache = LogoCache::new();
        let logo = cache.prepare(&data, 0);
        assert_eq!(logo.status, "projects.logo_base64_image");
        assert!(logo.pixels.as_slice().iter().any(|pixel| pixel.a != 0));
        assert!(cache.svg_options.as_ref().unwrap().fontdb.is_empty());
    }

    #[test]
    fn svg_raster_is_bounded_and_invalid_sources_fall_back() {
        let mut cache = LogoCache::new();
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="5000"><rect width="10000" height="5000" fill="#ff0000"/></svg>"##;
        let data = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        );
        let logo = cache.prepare(&data, 0);
        assert_eq!((logo.pixels.width(), logo.pixels.height()), (128, 64));
        assert_eq!(logo.pixels.as_slice()[0].r, 255);
        assert_eq!(
            cache.prepare("bad://logo", 0).status,
            "projects.invalid_logo_default"
        );
    }
}
