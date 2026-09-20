//! Lightweight selectable-text geometry for Chat message bodies.
//!
//! Message rows stay as plain Slint `Text`. Parley, the same shaping engine
//! used by Slint, supplies hit testing and selection rectangles for only the
//! active message without introducing a `TextInput` into row layout.

use crate::{ChatSelectionRect, MainWindow};
use parley::editing::{Cursor, Selection};
use parley::fontique::GenericFamily;
#[cfg(not(test))]
use parley::fontique::SourceCache;
use parley::style::{
    FontFamily, FontFamilyName, FontStyle, FontWeight, OverflowWrap, TextWrapMode, WordBreak,
};
use parley::{Affinity, FontContext, Layout, LayoutContext, StyleProperty};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;

const GEOMETRY_CACHE_CAPACITY: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeometryKey {
    text: String,
    family: String,
    font_size_bits: u32,
    scale_bits: u32,
    wrap_width_bits: u32,
}

struct CachedLayout {
    key: GeometryKey,
    layout: Layout<[u8; 4]>,
}

struct TextGeometry {
    font_context: FontContext,
    layout_context: LayoutContext<[u8; 4]>,
    cache: VecDeque<CachedLayout>,
}

impl TextGeometry {
    fn new() -> Self {
        Self {
            font_context: chat_font_context(),
            layout_context: LayoutContext::new(),
            cache: VecDeque::new(),
        }
    }

    fn with_layout<R>(
        &mut self,
        text: &str,
        width: f32,
        family: &str,
        font_size: f32,
        scale_factor: f32,
        operation: impl FnOnce(&Layout<[u8; 4]>) -> R,
    ) -> R {
        let key = GeometryKey {
            text: text.to_owned(),
            family: family.to_owned(),
            font_size_bits: font_size.to_bits(),
            scale_bits: scale_factor.to_bits(),
            wrap_width_bits: width.to_bits(),
        };
        if let Some(index) = self.cache.iter().position(|entry| entry.key == key) {
            let entry = self.cache.remove(index).expect("cached Chat text layout");
            let result = operation(&entry.layout);
            self.cache.push_front(entry);
            return result;
        }

        let scale = scale_factor.max(0.1);
        let families = [
            FontFamilyName::named(family),
            FontFamilyName::Generic(GenericFamily::SansSerif),
            FontFamilyName::Generic(GenericFamily::SystemUi),
        ];
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, text, scale, false);
        builder.push_default(FontFamily::List(Cow::Borrowed(&families)));
        builder.push_default(StyleProperty::FontSize(font_size.max(1.0)));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(400.0)));
        builder.push_default(StyleProperty::FontStyle(FontStyle::Normal));
        builder.push_default(StyleProperty::WordBreak(WordBreak::Normal));
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::Wrap));
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(width.max(1.0) * scale));

        let result = operation(&layout);
        self.cache.push_front(CachedLayout { key, layout });
        self.cache.truncate(GEOMETRY_CACHE_CAPACITY);
        result
    }
}

#[cfg(not(test))]
fn chat_font_context() -> FontContext {
    FontContext {
        collection: slint::fontique_011::shared_collection(),
        source_cache: SourceCache::default(),
    }
}

#[cfg(test)]
fn chat_font_context() -> FontContext {
    FontContext::new()
}

thread_local! {
    static TEXT_GEOMETRY: RefCell<Option<TextGeometry>> = const { RefCell::new(None) };
}

pub(super) fn measure_wrapped_system_text_height(text: &str, width: f32, font_size: f32) -> f32 {
    if text.is_empty() {
        return font_size.max(1.0);
    }
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(TextGeometry::new);
        engine.with_layout(text, width, "system-ui", font_size, 1.0, |layout| {
            layout.height().max(font_size.max(1.0))
        })
    })
}

pub(super) fn setup(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_chat_resolve_text_hit(move |text, x, y, width, line_pitch, family, font_size| {
        let scale_factor = ui_weak
            .upgrade()
            .map_or(1.0, |ui| ui.window().scale_factor());

        hit_test(
            text.as_str(),
            x,
            y,
            width,
            line_pitch,
            family.as_str(),
            font_size,
            scale_factor,
        )
    });

    let ui_weak = ui.as_weak();
    ui.on_chat_resolve_selection_rects(
        move |text, start, end, width, line_pitch, family, font_size| {
            let scale_factor = ui_weak
                .upgrade()
                .map_or(1.0, |ui| ui.window().scale_factor());
            ModelRc::new(VecModel::from(selection_rects(
                text.as_str(),
                start,
                end,
                width,
                line_pitch,
                family.as_str(),
                font_size,
                scale_factor,
            )))
        },
    );

    ui.on_chat_copy_selection(move |text, start, end| {
        let selected = selected_text(text.as_str(), start, end);
        if selected.is_empty() {
            return false;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(selected)) {
            Ok(()) => true,
            Err(error) => {
                log::error!("[Chat::Selection] Clipboard write failed: {error}");
                false
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn hit_test(
    text: &str,
    pointer_x: f32,
    pointer_y: f32,
    width: f32,
    line_pitch: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> i32 {
    if text.is_empty() || width <= 0.0 {
        return 0;
    }
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(TextGeometry::new);
        engine.with_layout(text, width, family, font_size, scale_factor, |layout| {
            let scale = scale_factor.max(0.1);
            let line_count = layout.lines().count().max(1);
            let visual_line = (pointer_y.max(0.0) / line_pitch.max(1.0)).floor() as usize;
            let line = layout
                .lines()
                .nth(visual_line.min(line_count - 1))
                .expect("Chat text layout has a line");
            let metrics = line.metrics();
            let y = (metrics.block_min_coord + metrics.block_max_coord) * 0.5;
            let byte = Cursor::from_point(layout, pointer_x.max(0.0) * scale, y).index();
            text[..byte.min(text.len())].chars().count() as i32
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn selection_rects(
    text: &str,
    start: i32,
    end: i32,
    width: f32,
    line_pitch: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> Vec<ChatSelectionRect> {
    let (lo, hi) = normalized_char_range(text, start, end);
    if text.is_empty() || width <= 0.0 || lo == hi {
        return Vec::new();
    }
    let start_byte = byte_offset_at_char(text, lo);
    let end_byte = byte_offset_at_char(text, hi);
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(TextGeometry::new);
        engine.with_layout(text, width, family, font_size, scale_factor, |layout| {
            let scale = scale_factor.max(0.1);
            let rects = Selection::new(
                Cursor::from_byte_index(layout, start_byte, Affinity::Downstream),
                Cursor::from_byte_index(layout, end_byte, Affinity::Upstream),
            )
            .geometry(layout)
            .into_iter()
            .map(|(bounds, line_index)| ChatSelectionRect {
                x: bounds.x0 as f32 / scale,
                y: line_index as f32 * line_pitch.max(1.0),
                width: (bounds.width() as f32 / scale).max(1.0),
                height: line_pitch.max(1.0),
            })
            .collect();
            coalesce_visual_line_rects(rects)
        })
    })
}

fn coalesce_visual_line_rects(mut rects: Vec<ChatSelectionRect>) -> Vec<ChatSelectionRect> {
    const TOUCH_EPSILON: f32 = 0.5;
    rects.sort_by(|left, right| {
        left.y
            .total_cmp(&right.y)
            .then_with(|| left.x.total_cmp(&right.x))
    });
    let mut merged: Vec<ChatSelectionRect> = Vec::with_capacity(rects.len());
    for rect in rects {
        if let Some(previous) = merged.last_mut() {
            let previous_right = previous.x + previous.width;
            if (previous.y - rect.y).abs() < 0.01 && rect.x <= previous_right + TOUCH_EPSILON {
                previous.width = previous_right.max(rect.x + rect.width) - previous.x;
                previous.height = previous.height.max(rect.height);
                continue;
            }
        }
        merged.push(rect);
    }
    merged
}

fn selected_text(text: &str, start: i32, end: i32) -> String {
    let (lo, hi) = normalized_char_range(text, start, end);
    text.chars().skip(lo).take(hi.saturating_sub(lo)).collect()
}

fn normalized_char_range(text: &str, start: i32, end: i32) -> (usize, usize) {
    let char_count = text.chars().count() as i32;
    (
        start.min(end).clamp(0, char_count) as usize,
        start.max(end).clamp(0, char_count) as usize,
    )
}

fn byte_offset_at_char(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_text_is_unicode_safe_in_both_directions() {
        assert_eq!(selected_text("a中文z", 1, 3), "中文");
        assert_eq!(selected_text("a中文z", 3, 1), "中文");
    }

    #[test]
    fn empty_or_collapsed_selection_has_no_rectangles() {
        assert!(selection_rects("text", 2, 2, 200.0, 18.0, "sans-serif", 14.0, 1.0).is_empty());
        assert!(selection_rects("", 0, 1, 200.0, 18.0, "sans-serif", 14.0, 1.0).is_empty());
    }

    #[test]
    fn reverse_selection_normalizes_to_the_same_range() {
        assert_eq!(normalized_char_range("alpha beta", 1, 8), (1, 8));
        assert_eq!(normalized_char_range("alpha beta", 8, 1), (1, 8));
    }
}
