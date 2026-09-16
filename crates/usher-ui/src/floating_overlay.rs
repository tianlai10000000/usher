//! Native rectangle adapter for Floating UI. No placement algorithm lives here.

use std::{cell::RefCell, rc::Rc};

use floating_ui_core::{
    ComputePositionConfig, DetectOverflowOptions, GetClippingRectArgs, GetElementRectsArgs,
    Platform, compute_position,
    middleware::{
        ApplyState, CrossAxis, FallbackStrategy, Flip, FlipOptions, Offset, OffsetOptions, Shift,
        ShiftOptions, Size, SizeOptions,
    },
};
use floating_ui_utils::{
    Alignment, Dimensions, ElementOrVirtual, ElementRects, Padding, Placement, Rect, Side, get_side,
};
use slint::ComponentHandle;

use crate::{FloatingOverlayBridge, FloatingOverlayRect, MainWindow};

#[derive(Clone, Debug, PartialEq)]
struct Surface(Rc<RefCell<Rect>>);

#[derive(Debug)]
struct NativePlatform {
    anchor: Rect,
    boundary: Rect,
}

impl Platform<Surface, ()> for NativePlatform {
    fn get_element_rects(&self, args: GetElementRectsArgs<Surface>) -> ElementRects {
        ElementRects {
            reference: self.anchor.clone(),
            floating: args.floating.0.borrow().clone(),
        }
    }

    fn get_clipping_rect(&self, _: GetClippingRectArgs<Surface>) -> Rect {
        self.boundary.clone()
    }

    fn get_dimensions(&self, surface: &Surface) -> Dimensions {
        let rect = surface.0.borrow();
        Dimensions {
            width: rect.width,
            height: rect.height,
        }
    }
}

fn apply_size(state: ApplyState<Surface, ()>) {
    let surface = &state.state.elements.floating.0;
    let mut rect = surface.borrow_mut();
    rect.width = rect.width.min(state.available_width.max(0.0));
    rect.height = rect.height.min(state.available_height.max(0.0));
}

fn rect(value: FloatingOverlayRect) -> Rect {
    Rect {
        x: value.x as f64,
        y: value.y as f64,
        width: value.width.max(0.0) as f64,
        height: value.height.max(0.0) as f64,
    }
}

fn position(
    anchor: FloatingOverlayRect,
    boundary: FloatingOverlayRect,
    size: FloatingOverlayRect,
    gap: f32,
    padding: f32,
    placement: Placement,
) -> FloatingOverlayRect {
    let platform = NativePlatform {
        anchor: rect(anchor),
        boundary: rect(boundary),
    };
    let surface = Surface(Rc::new(RefCell::new(rect(size))));
    let reference = Surface(Rc::new(RefCell::new(platform.anchor.clone())));
    let overflow = DetectOverflowOptions::default().padding(Padding::All(padding.max(0.0) as f64));
    // Primer tries alternate sides before alternate alignments. Keep geometry
    // in Floating UI, configuring its two Flip passes from Primer's order:
    // https://github.com/primer/behaviors/blob/main/src/anchored-position.ts
    let align = floating_ui_utils::get_alignment(placement);
    let aligned = |side| Placement::from((side, align));
    let side_result = compute_position(
        ElementOrVirtual::Element(&reference),
        &surface,
        ComputePositionConfig::new(&platform)
            .placement(placement)
            .middleware(vec![
                Box::new(Offset::new(OffsetOptions::Value(gap as f64))),
                Box::new(Flip::new(
                    FlipOptions::default()
                        .detect_overflow(overflow.clone())
                        .cross_axis(CrossAxis::False)
                        .fallback_placements(vec![
                            aligned(if get_side(placement) == Side::Top {
                                Side::Bottom
                            } else {
                                Side::Top
                            }),
                            aligned(Side::Right),
                            aligned(Side::Left),
                            aligned(Side::Bottom),
                        ])
                        .fallback_strategy(FallbackStrategy::InitialPlacement),
                )),
            ]),
    );
    let side = get_side(side_result.placement);
    let alignments = match align {
        Some(Alignment::Start) => [Some(Alignment::End), None],
        Some(Alignment::End) => [Some(Alignment::Start), None],
        None => [Some(Alignment::End), Some(Alignment::Start)],
    };
    let result = compute_position(
        ElementOrVirtual::Element(&reference),
        &surface,
        ComputePositionConfig::new(&platform)
            .placement(side_result.placement)
            .middleware(vec![
                Box::new(Offset::new(OffsetOptions::Value(gap as f64))),
                Box::new(Flip::new(
                    FlipOptions::default()
                        .detect_overflow(overflow.clone())
                        .main_axis(false)
                        .fallback_placements(
                            alignments
                                .map(|align| Placement::from((side, align)))
                                .to_vec(),
                        )
                        .fallback_strategy(FallbackStrategy::InitialPlacement),
                )),
                Box::new(Shift::new(
                    ShiftOptions::default()
                        .detect_overflow(overflow.clone())
                        .cross_axis(true),
                )),
                Box::new(Size::new(
                    SizeOptions::default()
                        .detect_overflow(overflow)
                        .apply(&apply_size),
                )),
            ]),
    );
    let size = surface.0.borrow();
    FloatingOverlayRect {
        x: result.x as f32,
        y: result.y as f32,
        width: size.width as f32,
        height: size.height as f32,
    }
}

/// Install on the Slint event loop before any adaptive menu is opened.
pub fn install_floating_overlay_positioning(window: &MainWindow) {
    let bridge = window.global::<FloatingOverlayBridge>();
    bridge.on_position(|anchor, boundary, size, gap, padding, side, align| {
        let side = if side == "top" {
            Side::Top
        } else {
            Side::Bottom
        };
        let align = match align.as_str() {
            "end" => Some(Alignment::End),
            "center" => None,
            _ => Some(Alignment::Start),
        };
        position(
            anchor,
            boundary,
            size,
            gap,
            padding,
            Placement::from((side, align)),
        )
    });
    bridge.set_ready(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> FloatingOverlayRect {
        FloatingOverlayRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn primer_defaults_preserve_start_alignment_and_four_pixel_gap() {
        let viewport = bounds(0.0, 0.0, 800.0, 600.0);
        let size = bounds(0.0, 0.0, 200.0, 160.0);
        let normal = position(
            bounds(300.0, 100.0, 80.0, 32.0),
            viewport.clone(),
            size.clone(),
            4.0,
            8.0,
            Placement::BottomStart,
        );
        assert_eq!((normal.x, normal.y), (300.0, 136.0));
        let right = position(
            bounds(700.0, 100.0, 80.0, 32.0),
            viewport.clone(),
            size.clone(),
            4.0,
            8.0,
            Placement::BottomStart,
        );
        assert_eq!((right.x, right.y), (580.0, 136.0));
        let above = position(
            bounds(300.0, 540.0, 80.0, 32.0),
            viewport,
            size,
            4.0,
            8.0,
            Placement::BottomStart,
        );
        assert_eq!((above.x, above.y), (300.0, 376.0));
    }

    #[test]
    fn follows_github_anchor_and_switches_alignment_at_the_left_edge() {
        let viewport = bounds(20.0, 30.0, 800.0, 600.0);
        let size = bounds(0.0, 0.0, 200.0, 160.0);
        let normal = position(
            bounds(500.0, 100.0, 80.0, 32.0),
            viewport.clone(),
            size.clone(),
            4.0,
            8.0,
            Placement::BottomEnd,
        );
        assert_eq!((normal.x, normal.y), (380.0, 136.0));
        let left = position(
            bounds(30.0, 100.0, 80.0, 32.0),
            viewport,
            size,
            4.0,
            8.0,
            Placement::BottomEnd,
        );
        assert_eq!((left.x, left.y), (30.0, 136.0));
    }

    #[test]
    fn flips_up_then_uses_lateral_space_for_a_tall_menu() {
        let viewport = bounds(0.0, 0.0, 800.0, 400.0);
        let above = position(
            bounds(650.0, 340.0, 80.0, 32.0),
            viewport.clone(),
            bounds(0.0, 0.0, 200.0, 160.0),
            4.0,
            8.0,
            Placement::BottomEnd,
        );
        assert_eq!((above.x, above.y), (530.0, 176.0));
        let lateral = position(
            bounds(650.0, 200.0, 80.0, 32.0),
            viewport,
            bounds(0.0, 0.0, 200.0, 360.0),
            4.0,
            8.0,
            Placement::BottomEnd,
        );
        assert_eq!(lateral.x + lateral.width, 646.0);
        assert_eq!(lateral.height, 360.0);
        assert!(lateral.y >= 8.0 && lateral.y + lateral.height <= 392.0);
    }

    #[test]
    fn constrains_large_content_at_all_window_edges() {
        for (width, height) in [(320.0, 240.0), (640.0, 400.0), (1200.0, 900.0)] {
            for x in [8.0, width / 2.0, width - 88.0] {
                for y in [8.0, height / 2.0, height - 40.0] {
                    let menu = position(
                        bounds(x, y, 80.0, 32.0),
                        bounds(0.0, 0.0, width, height),
                        bounds(0.0, 0.0, 400.0, 600.0),
                        4.0,
                        8.0,
                        Placement::BottomEnd,
                    );
                    assert!(menu.x >= 7.99 && menu.y >= 7.99, "{menu:?}");
                    assert!(menu.x + menu.width <= width - 7.99, "{menu:?}");
                    assert!(menu.y + menu.height <= height - 7.99, "{menu:?}");
                    assert!(menu.width > 0.0 && menu.height > 0.0, "{menu:?}");
                }
            }
        }
    }
}
