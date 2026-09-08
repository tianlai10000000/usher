//! Exact styled-text hit testing shared by PR diff selection and Find paint.
//!
//! Parley is the shaping engine used by Slint 1.17. Keeping it behind this
//! module lets the UI depend on cursor geometry rather than a concrete engine.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;

use parley::editing::{Cursor, Selection};
use parley::fontique::{GenericFamily, SourceCache};
use parley::style::{
    FontFamily, FontFamilyName, FontStyle, FontWeight, OverflowWrap, TextWrapMode, WordBreak,
};
use parley::{Affinity, FontContext, Layout, LayoutContext, StyleProperty};

const GEOMETRY_CACHE_CAPACITY: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeometryKey {
    text: String,
    family: String,
    font_size_bits: u32,
    weight: u16,
    italic: bool,
    scale_bits: u32,
    wrap_width_bits: Option<u32>,
}

struct CachedLayout {
    key: GeometryKey,
    layout: Layout<[u8; 4]>,
}

struct ParleyTextGeometry {
    font_context: FontContext,
    layout_context: LayoutContext<[u8; 4]>,
    cache: VecDeque<CachedLayout>,
}

impl ParleyTextGeometry {
    fn new() -> Self {
        Self::with_font_context(FontContext {
            collection: slint::fontique_010::shared_collection(),
            source_cache: SourceCache::default(),
        })
    }

    fn with_font_context(font_context: FontContext) -> Self {
        Self {
            font_context,
            layout_context: LayoutContext::new(),
            cache: VecDeque::new(),
        }
    }

    // Parameters form the cache key for one shaped-text layout.
    #[allow(clippy::too_many_arguments)]
    fn with_layout<R>(
        &mut self,
        text: &str,
        family: &str,
        font_size: f32,
        weight: u16,
        italic: bool,
        scale_factor: f32,
        operation: impl FnOnce(&Layout<[u8; 4]>) -> R,
    ) -> R {
        self.with_layout_options(
            text,
            family,
            font_size,
            weight,
            italic,
            scale_factor,
            None,
            operation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_layout_options<R>(
        &mut self,
        text: &str,
        family: &str,
        font_size: f32,
        weight: u16,
        italic: bool,
        scale_factor: f32,
        wrap_width: Option<f32>,
        operation: impl FnOnce(&Layout<[u8; 4]>) -> R,
    ) -> R {
        let font_size_bits = font_size.to_bits();
        let scale_bits = scale_factor.to_bits();
        let wrap_width_bits = wrap_width.map(f32::to_bits);
        if let Some(index) = self.cache.iter().position(|entry| {
            entry.key.text == text
                && entry.key.family == family
                && entry.key.font_size_bits == font_size_bits
                && entry.key.weight == weight
                && entry.key.italic == italic
                && entry.key.scale_bits == scale_bits
                && entry.key.wrap_width_bits == wrap_width_bits
        }) {
            let entry = self.cache.remove(index).expect("cached text layout");
            let result = operation(&entry.layout);
            self.cache.push_front(entry);
            return result;
        }

        let key = GeometryKey {
            text: text.to_owned(),
            family: family.to_owned(),
            font_size_bits,
            weight,
            italic,
            scale_bits,
            wrap_width_bits,
        };

        let families = [
            FontFamilyName::named(family),
            FontFamilyName::Generic(GenericFamily::SansSerif),
            FontFamilyName::Generic(GenericFamily::SystemUi),
        ];
        let mut builder = self.layout_context.ranged_builder(
            &mut self.font_context,
            text,
            scale_factor.max(0.1),
            false,
        );
        builder.push_default(FontFamily::List(Cow::Borrowed(&families)));
        builder.push_default(StyleProperty::FontSize(font_size.max(1.0)));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight as f32)));
        builder.push_default(StyleProperty::FontStyle(if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        if wrap_width.is_some() {
            builder.push_default(StyleProperty::WordBreak(WordBreak::Normal));
            builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
            builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::Wrap));
        } else {
            builder.push_default(StyleProperty::WordBreak(WordBreak::KeepAll));
            builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Normal));
            builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::NoWrap));
        }
        let mut layout = builder.build(text);
        layout.break_all_lines(wrap_width.map(|width| width.max(1.0) * scale_factor.max(0.1)));

        let result = operation(&layout);
        self.cache.push_front(CachedLayout { key, layout });
        self.cache.truncate(GEOMETRY_CACHE_CAPACITY);
        result
    }
}

thread_local! {
    static TEXT_GEOMETRY: RefCell<Option<ParleyTextGeometry>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TextRangeGeometry {
    pub x: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TextGlyphRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TextGeometryOptions<'a> {
    pub text: &'a str,
    pub rendered_width: f32,
    pub line_pitch: f32,
    pub family: &'a str,
    pub font_size: f32,
    pub weight: u16,
    pub italic: bool,
    pub scale_factor: f32,
}

pub(super) fn hit_test(options: TextGeometryOptions<'_>, pointer_x: f32) -> i32 {
    if options.text.is_empty() || options.rendered_width <= 0.0 {
        return 0;
    }
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(ParleyTextGeometry::new);
        engine.with_layout(
            options.text,
            options.family,
            options.font_size,
            options.weight,
            options.italic,
            options.scale_factor,
            |layout| char_index_at_point(layout, options.text, pointer_x, options.rendered_width),
        )
    })
}

/// Hit-test a line painted as adjacent, independently shaped Slint Text runs.
/// The iterator order must match the visible token row order.
pub(super) fn hit_test_runs<I, S>(
    runs: I,
    pointer_x: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> i32
where
    I: IntoIterator<Item = (S, u16, bool)>,
    S: AsRef<str>,
{
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(ParleyTextGeometry::new);
        hit_test_runs_with_engine(engine, runs, pointer_x, family, font_size, scale_factor)
    })
}

pub(super) fn hit_test_wrapped(
    options: TextGeometryOptions<'_>,
    pointer_x: f32,
    pointer_y: f32,
) -> i32 {
    if options.text.is_empty() {
        return 0;
    }
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(ParleyTextGeometry::new);
        hit_test_wrapped_with_engine(
            engine,
            options.text,
            pointer_x,
            pointer_y,
            options.rendered_width,
            options.line_pitch,
            options.family,
            options.font_size,
            options.scale_factor,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn hit_test_wrapped_with_engine(
    engine: &mut ParleyTextGeometry,
    text: &str,
    pointer_x: f32,
    pointer_y: f32,
    rendered_width: f32,
    line_pitch: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> i32 {
    engine.with_layout_options(
        text,
        family,
        font_size,
        400,
        false,
        scale_factor,
        Some(rendered_width),
        |layout| {
            let line_count = layout.lines().count().max(1);
            let visual_line = (pointer_y.max(0.0) / line_pitch.max(1.0)).floor() as usize;
            let line = layout
                .lines()
                .nth(visual_line.min(line_count - 1))
                .expect("wrapped text layout has a line");
            let metrics = line.metrics();
            let y = (metrics.block_min_coord + metrics.block_max_coord) * 0.5;
            char_index_at_layout_point(layout, text, pointer_x.max(0.0) * scale_factor.max(0.1), y)
        },
    )
}

fn hit_test_runs_with_engine<I, S>(
    engine: &mut ParleyTextGeometry,
    runs: I,
    pointer_x: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> i32
where
    I: IntoIterator<Item = (S, u16, bool)>,
    S: AsRef<str>,
{
    let scale = scale_factor.max(0.1);
    let target_x = pointer_x.max(0.0) * scale;
    let mut advance = 0.0_f32;
    let mut preceding_chars = 0_i32;

    for (text, weight, italic) in runs {
        let text = text.as_ref();
        let char_count = text.chars().count() as i32;
        let hit = engine.with_layout(text, family, font_size, weight, italic, scale, |layout| {
            let width = layout.full_width().max(0.0);
            let local_x = target_x - advance;
            let contains_pointer = local_x <= width;
            (
                width,
                contains_pointer.then(|| char_index_at_layout_x(layout, text, local_x)),
            )
        });
        if let Some(local_char) = hit.1 {
            return preceding_chars + local_char;
        }
        advance += hit.0;
        preceding_chars += char_count;
    }

    preceding_chars
}

pub(super) fn range_geometry(
    options: TextGeometryOptions<'_>,
    start_char: i32,
    end_char: i32,
) -> TextRangeGeometry {
    if options.text.is_empty() || options.rendered_width <= 0.0 || end_char <= start_char {
        return TextRangeGeometry::default();
    }
    let char_count = options.text.chars().count() as i32;
    let start = start_char.clamp(0, char_count) as usize;
    let end = end_char.clamp(start as i32, char_count) as usize;
    let start_byte = byte_offset_at_char(options.text, start);
    let end_byte = byte_offset_at_char(options.text, end);

    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(ParleyTextGeometry::new);
        engine.with_layout(
            options.text,
            options.family,
            options.font_size,
            options.weight,
            options.italic,
            options.scale_factor,
            |layout| range_in_layout(layout, start_byte, end_byte, options.rendered_width),
        )
    })
}

pub(super) fn wrapped_range_geometry(
    options: TextGeometryOptions<'_>,
    start_char: i32,
    end_char: i32,
) -> Vec<TextGlyphRect> {
    if options.text.is_empty() || options.rendered_width <= 0.0 || end_char <= start_char {
        return Vec::new();
    }
    TEXT_GEOMETRY.with_borrow_mut(|slot| {
        let engine = slot.get_or_insert_with(ParleyTextGeometry::new);
        wrapped_range_geometry_with_engine(
            engine,
            options.text,
            start_char,
            end_char,
            options.rendered_width,
            options.line_pitch,
            options.family,
            options.font_size,
            options.scale_factor,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn wrapped_range_geometry_with_engine(
    engine: &mut ParleyTextGeometry,
    text: &str,
    start_char: i32,
    end_char: i32,
    rendered_width: f32,
    line_pitch: f32,
    family: &str,
    font_size: f32,
    scale_factor: f32,
) -> Vec<TextGlyphRect> {
    let char_count = text.chars().count() as i32;
    let start = start_char.clamp(0, char_count) as usize;
    let end = end_char.clamp(start as i32, char_count) as usize;
    if start == end {
        return Vec::new();
    }
    let start_byte = byte_offset_at_char(text, start);
    let end_byte = byte_offset_at_char(text, end);
    let scale = scale_factor.max(0.1);
    let pitch = line_pitch.max(1.0);

    engine.with_layout_options(
        text,
        family,
        font_size,
        400,
        false,
        scale,
        Some(rendered_width),
        |layout| {
            let rects = Selection::new(
                Cursor::from_byte_index(layout, start_byte, Affinity::Downstream),
                Cursor::from_byte_index(layout, end_byte, Affinity::Upstream),
            )
            .geometry(layout)
            .into_iter()
            .map(|(bounds, line_index)| TextGlyphRect {
                x: bounds.x0 as f32 / scale,
                y: line_index as f32 * pitch,
                width: (bounds.width() as f32 / scale).max(1.0),
                height: pitch,
            })
            .collect();
            coalesce_visual_line_rects(rects)
        },
    )
}

fn coalesce_visual_line_rects(mut rects: Vec<TextGlyphRect>) -> Vec<TextGlyphRect> {
    const TOUCH_EPSILON: f32 = 0.5;
    rects.sort_by(|left, right| {
        left.y
            .total_cmp(&right.y)
            .then_with(|| left.x.total_cmp(&right.x))
    });
    let mut merged: Vec<TextGlyphRect> = Vec::with_capacity(rects.len());
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

fn byte_offset_at_char(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

fn char_index_at_point(
    layout: &Layout<[u8; 4]>,
    text: &str,
    pointer_x: f32,
    rendered_width: f32,
) -> i32 {
    let layout_width = layout.full_width().max(0.1);
    let x = pointer_x.clamp(0.0, rendered_width) * layout_width / rendered_width;
    char_index_at_layout_x(layout, text, x)
}

fn char_index_at_layout_x(layout: &Layout<[u8; 4]>, text: &str, x: f32) -> i32 {
    char_index_at_layout_point(layout, text, x, 0.0)
}

fn char_index_at_layout_point(layout: &Layout<[u8; 4]>, text: &str, x: f32, y: f32) -> i32 {
    let byte = Cursor::from_point(layout, x.max(0.0), y.max(0.0)).index();
    text[..byte.min(text.len())].chars().count() as i32
}

fn range_in_layout(
    layout: &Layout<[u8; 4]>,
    start_byte: usize,
    end_byte: usize,
    rendered_width: f32,
) -> TextRangeGeometry {
    let cursor_width = 0.0;
    let start_x = Cursor::from_byte_index(layout, start_byte, Affinity::Downstream)
        .geometry(layout, cursor_width)
        .x0;
    let end_x = Cursor::from_byte_index(layout, end_byte, Affinity::Downstream)
        .geometry(layout, cursor_width)
        .x0;
    let scale = f64::from(rendered_width / layout.full_width().max(0.1));
    let left = (start_x.min(end_x) * scale) as f32;
    let right = (start_x.max(end_x) * scale) as f32;
    TextRangeGeometry {
        x: left,
        width: (right - left).max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAMILY: &str = "Menlo";
    const SIZE: f32 = 13.0;

    fn with_test_layout<R>(
        engine: &mut ParleyTextGeometry,
        text: &str,
        weight: u16,
        italic: bool,
        operation: impl FnOnce(&Layout<[u8; 4]>) -> R,
    ) -> R {
        engine.with_layout(text, FAMILY, SIZE, weight, italic, 1.0, operation)
    }

    #[test]
    fn mixed_script_and_punctuation_hit_testing_uses_shaped_carets() {
        let text = "coding agent 使用：(event-loop) => true;";
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        with_test_layout(&mut engine, text, 400, false, |layout| {
            let rendered_width = layout.full_width();
            let boundaries = text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(text.len()))
                .collect::<Vec<_>>();

            for (char_index, bytes) in boundaries.windows(2).enumerate() {
                let glyph = range_in_layout(layout, bytes[0], bytes[1], rendered_width);
                assert_eq!(
                    char_index_at_point(layout, text, glyph.x + glyph.width * 0.25, rendered_width,),
                    char_index as i32,
                    "left half of character {char_index}"
                );
                assert_eq!(
                    char_index_at_point(layout, text, glyph.x + glyph.width * 0.75, rendered_width,),
                    char_index as i32 + 1,
                    "right half of character {char_index}"
                );
            }
        });
    }

    #[test]
    fn full_mixed_range_reaches_both_shaped_edges() {
        let text = "Mission / 目标 — `reply?`";
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        with_test_layout(&mut engine, text, 600, true, |layout| {
            let rendered_width = layout.full_width() * 1.37;
            let range = range_in_layout(layout, 0, text.len(), rendered_width);
            assert!(range.x.abs() < 0.01);
            assert!((range.width - rendered_width).abs() < 0.01);
        });
    }

    #[test]
    fn style_is_part_of_the_bounded_layout_cache_key() {
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        for index in 0..(GEOMETRY_CACHE_CAPACITY + 32) {
            let text = format!("token-{index}");
            with_test_layout(
                &mut engine,
                &text,
                if index % 2 == 0 { 400 } else { 600 },
                index % 3 == 0,
                |_| (),
            );
        }
        assert_eq!(engine.cache.len(), GEOMETRY_CACHE_CAPACITY);

        with_test_layout(&mut engine, "same", 400, false, |_| ());
        with_test_layout(&mut engine, "same", 600, false, |_| ());
        assert!(engine.cache.iter().any(|entry| entry.key.weight == 400));
        assert!(engine.cache.iter().any(|entry| entry.key.weight == 600));
    }

    #[test]
    fn adjacent_styled_runs_use_shaped_advances_for_cross_token_hits() {
        let runs = [
            ("**粗体**", 600, false),
            (" / ", 400, false),
            ("event-loop", 400, true),
        ];
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        let first_two_width = runs[..2]
            .iter()
            .map(|(text, weight, italic)| {
                with_test_layout(&mut engine, text, *weight, *italic, |layout| {
                    layout.full_width()
                })
            })
            .sum::<f32>();
        let third_char = with_test_layout(&mut engine, runs[2].0, 400, true, |layout| {
            range_in_layout(layout, 0, "e".len(), layout.full_width())
        });
        let hit = hit_test_runs_with_engine(
            &mut engine,
            runs,
            first_two_width + third_char.x + third_char.width * 0.75,
            FAMILY,
            SIZE,
            1.0,
        );
        let preceding = runs[..2]
            .iter()
            .map(|(text, _, _)| text.chars().count() as i32)
            .sum::<i32>();
        assert_eq!(hit, preceding + 1);
    }

    #[test]
    fn wrapped_cjk_range_stops_before_following_space() {
        let text = "前缀 保持 后缀";
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        let single = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            3,
            4,
            500.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        let following_character = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            4,
            5,
            500.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        let following_space = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            5,
            6,
            500.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        assert_eq!(single.len(), 1);
        assert_eq!(following_character.len(), 1);
        assert_eq!(following_space.len(), 1);
        let single_right = single[0].x + single[0].width;
        assert!(
            (single_right - following_character[0].x).abs() < 0.01,
            "single glyph ends at {single_right}, next glyph starts at {}",
            following_character[0].x
        );

        let pair = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            3,
            5,
            500.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        assert_eq!(pair.len(), 1);
        assert!((pair[0].x + pair[0].width - following_space[0].x).abs() < 0.01);
    }

    #[test]
    fn wrapped_range_splits_at_real_visual_line_boundaries() {
        let text = "abcdefghij";
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        let four_chars_width = with_test_layout(&mut engine, "abcd", 400, false, |layout| {
            layout.full_width()
        });
        let rects = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            3,
            7,
            four_chars_width + 0.1,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        assert!(rects.len() >= 2, "rects={rects:?}");
        assert!(rects.windows(2).any(|pair| pair[0].y < pair[1].y));
        for pair in rects.windows(2).filter(|pair| pair[0].y < pair[1].y) {
            assert!(
                pair[0].y + pair[0].height <= pair[1].y + 0.01,
                "adjacent visual-line rectangles overlap: {pair:?}"
            );
        }
    }

    #[test]
    fn second_visual_line_hit_uses_shaped_punctuation_geometry() {
        let text = "abcdefghij、保持 klmnop";
        let target = text
            .chars()
            .position(|character| character == '保')
            .unwrap() as i32;
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        let rect = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            target,
            target + 1,
            64.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        )
        .into_iter()
        .next()
        .expect("target glyph geometry");
        assert!(
            rect.y > 0.0,
            "target must be on a continuation line: {rect:?}"
        );
        let left = hit_test_wrapped_with_engine(
            &mut engine,
            text,
            rect.x + rect.width * 0.25,
            rect.y + rect.height * 0.5,
            64.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        let right = hit_test_wrapped_with_engine(
            &mut engine,
            text,
            rect.x + rect.width * 0.75,
            rect.y + rect.height * 0.5,
            64.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        assert_eq!(left, target);
        assert_eq!(right, target + 1);
    }

    #[test]
    fn wrapped_geometry_is_scale_independent_in_logical_pixels() {
        let text = "中文、 punctuation 保持";
        let start = text
            .chars()
            .position(|character| character == '保')
            .unwrap() as i32;
        let mut engine = ParleyTextGeometry::with_font_context(FontContext::new());
        let one = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            start,
            start + 2,
            96.0,
            18.0,
            FAMILY,
            SIZE,
            1.0,
        );
        let two = wrapped_range_geometry_with_engine(
            &mut engine,
            text,
            start,
            start + 2,
            96.0,
            18.0,
            FAMILY,
            SIZE,
            2.0,
        );
        assert_eq!(one.len(), two.len());
        for (one, two) in one.iter().zip(&two) {
            assert!((one.x - two.x).abs() < 0.05, "{one:?} != {two:?}");
            assert!((one.y - two.y).abs() < 0.05, "{one:?} != {two:?}");
            assert!((one.width - two.width).abs() < 0.05, "{one:?} != {two:?}");
        }
    }

    #[test]
    fn adjacent_font_run_fragments_merge_without_joining_real_gaps() {
        let merged = coalesce_visual_line_rects(vec![
            TextGlyphRect {
                x: 24.0,
                y: 0.0,
                width: 5.0,
                height: 18.0,
            },
            TextGlyphRect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 18.0,
            },
            TextGlyphRect {
                x: 11.9,
                y: 0.0,
                width: 6.1,
                height: 18.0,
            },
            TextGlyphRect {
                x: 0.0,
                y: 18.0,
                width: 8.0,
                height: 18.0,
            },
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(
            merged[0],
            TextGlyphRect {
                x: 0.0,
                y: 0.0,
                width: 18.0,
                height: 18.0
            }
        );
        assert_eq!(
            merged[1],
            TextGlyphRect {
                x: 24.0,
                y: 0.0,
                width: 5.0,
                height: 18.0
            }
        );
        assert_eq!(
            merged[2],
            TextGlyphRect {
                x: 0.0,
                y: 18.0,
                width: 8.0,
                height: 18.0
            }
        );
    }
}
