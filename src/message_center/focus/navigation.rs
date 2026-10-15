//! Mounted-host Tab navigation across static ComponentFactory boundaries.

use i_slint_core::accessibility::AccessibleStringProperty;
use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::{
    ItemRc, ItemVisitorResult, ParentItemTraversalMode, TraversalOrder, visit_items,
};
use i_slint_core::items::{Clip, Flickable, FocusScope, Opacity, TextInput};
use i_slint_core::lengths::{LogicalLength, LogicalPoint, LogicalRect, LogicalSize};
use i_slint_core::window::WindowInner;
use slint::ComponentHandle;

use crate::MainWindow;

struct Target {
    item: ItemRc,
    geometry: LogicalRect,
    viewport: Option<(ItemRc, LogicalRect)>,
}

impl Target {
    fn reveal(&self) {
        let Some((viewport, bounds)) = &self.viewport else {
            return;
        };
        let Some(flickable) = viewport.downcast::<Flickable>() else {
            return;
        };
        let flickable = flickable.as_pin_ref();
        let delta = if self.geometry.min_y() < bounds.min_y() {
            bounds.min_y() - self.geometry.min_y()
        } else if self.geometry.max_y() > bounds.max_y() {
            bounds.max_y() - self.geometry.max_y()
        } else {
            return;
        };
        let minimum = (bounds.height() - flickable.content_height().0).min(0.0);
        let y = (flickable.content_y().0 + delta).clamp(minimum, 0.0);
        Flickable::FIELD_OFFSETS
            .content_y()
            .apply_pin(flickable)
            .set(LogicalLength::new(y));
    }
}

fn accepts_tab(item: &ItemRc) -> bool {
    let mut ancestor = Some(item.clone());
    while let Some(item) = ancestor {
        if item
            .accessible_string_property(AccessibleStringProperty::Enabled)
            .is_some_and(|enabled| enabled == "false")
        {
            return false;
        }
        ancestor = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    item.downcast::<FocusScope>().is_some_and(|scope| {
        let scope = scope.as_pin_ref();
        scope.enabled() && scope.focus_on_tab_navigation()
    }) || item
        .downcast::<TextInput>()
        .is_some_and(|input| input.as_pin_ref().enabled())
}

pub(super) fn move_tab(window: &MainWindow, backward: bool) -> bool {
    let host = WindowInner::from_pub(window.window());
    let Some(current) = host.focus_item.borrow().upgrade() else {
        return false;
    };
    let mut chain = Vec::new();
    let size = window
        .window()
        .size()
        .to_logical(window.window().scale_factor());
    let viewport = LogicalRect::new(
        LogicalPoint::default(),
        LogicalSize::new(size.width, size.height),
    );
    // Walk the mounted host so factory boundaries retain their surrounding
    // controls. Local ItemRc::next_focus_item wraps at the private factory root.
    // Retain offscreen rows with their actual mounted scroll viewport.
    visit_items(
        &host.component(),
        TraversalOrder::BackToFront,
        |tree, _, index, (origin, opacity, scroll_viewport)| {
            let item = ItemRc::new(tree.clone(), index);
            let geometry = item.geometry();
            let origin = *origin + geometry.origin.to_vector();
            let opacity = *opacity
                * item
                    .downcast::<Opacity>()
                    .map_or(1.0, |item| item.as_pin_ref().opacity());
            if item.downcast::<Clip>().is_some_and(|clip| {
                let clip = clip.as_pin_ref();
                clip.is_visibility_clip() && clip.clip()
            }) || opacity <= 0.0
            {
                return ItemVisitorResult::SkipChildren;
            }
            let geometry = LogicalRect::new(origin, geometry.size);
            let on_screen = viewport.intersects(&geometry);
            let scroll_viewport = if item.downcast::<Flickable>().is_some() && on_screen {
                Some((item.clone(), geometry))
            } else {
                scroll_viewport.clone()
            };
            if (accepts_tab(&item) || item == current)
                && (on_screen || scroll_viewport.is_some())
                && geometry.size.width > 0.0
                && geometry.size.height > 0.0
            {
                chain.push(Target {
                    item,
                    geometry,
                    viewport: scroll_viewport.clone(),
                });
            }
            ItemVisitorResult::Continue((origin, opacity, scroll_viewport))
        },
        (LogicalPoint::default(), 1.0f32, None),
    );
    let Some(index) = chain.iter().position(|target| target.item == current) else {
        return false;
    };
    let next = if backward {
        (index + chain.len() - 1) % chain.len()
    } else {
        (index + 1) % chain.len()
    };
    // Core checks the nearest clipping ancestor before revealing Tab focus.
    // An offscreen row's own Clip would reject a reverse target, then core's
    // forward fallback would loop locally. Reveal its exact viewport first.
    chain[next].reveal();
    host.set_focus_item(&chain[next].item, true, FocusReason::TabNavigation);
    true
}
