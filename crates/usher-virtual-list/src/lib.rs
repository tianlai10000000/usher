//! Framework-neutral variable-height virtual-list facade.
//!
//! Consumers depend on [`VirtualList`] and the stable data types in this
//! crate. The community engine is private so it can be replaced without
//! changing feature modules or UI bridges.

use std::sync::Arc;

use virtualizer::{Virtualizer, VirtualizerOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualItem {
    pub index: usize,
    pub start: u64,
    pub extent: u32,
}

impl VirtualItem {
    pub fn end(self) -> u64 {
        self.start.saturating_add(self.extent as u64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VirtualWindow {
    pub items: Vec<VirtualItem>,
    pub offset: u64,
    pub viewport_extent: u32,
    pub content_extent: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealTarget {
    pub index: usize,
    pub intra_item_offset: u32,
    pub extent: u32,
}

/// Replaceable engine port. Feature crates must not depend on a concrete
/// prefix-sum implementation or on third-party virtualizer types.
pub trait VirtualListEngine {
    fn replace_item_extents(&mut self, item_extents: &[u32], overscan: usize);
    fn set_viewport(&mut self, offset: u64, viewport_extent: u32);
    fn write_window(&self, output: &mut VirtualWindow);
    fn item(&self, index: usize) -> Option<VirtualItem>;
    fn maximum_offset(&self) -> u64;
}

pub struct VirtualList {
    engine: Box<dyn VirtualListEngine>,
}

impl VirtualList {
    pub fn new(item_extents: &[u32], overscan: usize) -> Self {
        Self::with_engine(
            Box::new(CommunityVirtualizerEngine::default()),
            item_extents,
            overscan,
        )
    }

    pub fn with_engine(
        mut engine: Box<dyn VirtualListEngine>,
        item_extents: &[u32],
        overscan: usize,
    ) -> Self {
        engine.replace_item_extents(item_extents, overscan);
        Self { engine }
    }

    pub fn replace_item_extents(&mut self, item_extents: &[u32], overscan: usize) {
        self.engine.replace_item_extents(item_extents, overscan);
    }

    pub fn set_viewport(&mut self, offset: u64, viewport_extent: u32) {
        self.engine.set_viewport(offset, viewport_extent);
    }

    pub fn window(&self) -> VirtualWindow {
        let mut output = VirtualWindow::default();
        self.write_window(&mut output);
        output
    }

    /// Projects the current window into caller-owned storage. Scroll bridges
    /// should prefer this method so the visible-item allocation is reused.
    pub fn write_window(&self, output: &mut VirtualWindow) {
        self.engine.write_window(output);
    }

    /// Blink-style CenterIfNeeded using exact prefix-sum item geometry.
    pub fn reveal_center_if_needed(&self, target: RevealTarget) -> u64 {
        let Some(item) = self.engine.item(target.index) else {
            return 0;
        };
        let mut window = VirtualWindow::default();
        self.engine.write_window(&mut window);
        let target_start = item
            .start
            .saturating_add(target.intra_item_offset.min(item.extent) as u64);
        let target_extent = target.extent.max(1) as u64;
        let target_end = target_start.saturating_add(target_extent);
        let viewport_end = window.offset.saturating_add(window.viewport_extent as u64);
        if target_start >= window.offset && target_end <= viewport_end {
            return window.offset;
        }
        let centered = target_start
            .saturating_add(target_extent / 2)
            .saturating_sub(window.viewport_extent as u64 / 2);
        centered.min(self.engine.maximum_offset())
    }
}

#[derive(Default)]
struct CommunityVirtualizerEngine {
    virtualizer: Option<Virtualizer>,
}

impl VirtualListEngine for CommunityVirtualizerEngine {
    fn replace_item_extents(&mut self, item_extents: &[u32], overscan: usize) {
        let extents = Arc::<[u32]>::from(item_extents);
        let options = VirtualizerOptions::new(extents.len(), move |index| {
            extents.get(index).copied().unwrap_or(1).max(1)
        })
        .with_overscan(overscan);
        self.virtualizer = Some(Virtualizer::new(options));
    }

    fn set_viewport(&mut self, offset: u64, viewport_extent: u32) {
        if let Some(virtualizer) = self.virtualizer.as_mut() {
            virtualizer.set_viewport_and_scroll_clamped(viewport_extent, offset);
        }
    }

    fn write_window(&self, output: &mut VirtualWindow) {
        let Some(virtualizer) = self.virtualizer.as_ref() else {
            output.items.clear();
            output.offset = 0;
            output.viewport_extent = 0;
            output.content_extent = 0;
            return;
        };
        output.items.clear();
        virtualizer.for_each_virtual_item(|item| {
            output.items.push(VirtualItem {
                index: item.index,
                start: item.start,
                extent: item.size,
            });
        });
        output.offset = virtualizer.scroll_offset();
        output.viewport_extent = virtualizer.scroll_rect().main;
        output.content_extent = virtualizer.total_size();
    }

    fn item(&self, index: usize) -> Option<VirtualItem> {
        let virtualizer = self.virtualizer.as_ref()?;
        Some(VirtualItem {
            index,
            start: virtualizer.item_start(index)?,
            extent: virtualizer.item_size(index)?,
        })
    }

    fn maximum_offset(&self) -> u64 {
        self.virtualizer
            .as_ref()
            .map_or(0, Virtualizer::max_scroll_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_extents_drive_total_size_and_visible_window() {
        let mut list = VirtualList::new(&[20, 40, 30, 50], 1);
        list.set_viewport(45, 30);
        let window = list.window();
        assert_eq!(window.content_extent, 140);
        assert_eq!(window.offset, 45);
        assert_eq!(
            window
                .items
                .iter()
                .map(|item| item.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(window.items[2].start, 60);
    }

    #[test]
    fn reveal_centers_an_intra_item_target_and_clamps_at_end() {
        let mut list = VirtualList::new(&[20, 80, 30], 1);
        list.set_viewport(0, 40);
        assert_eq!(
            list.reveal_center_if_needed(RevealTarget {
                index: 1,
                intra_item_offset: 50,
                extent: 10,
            }),
            55
        );
        assert_eq!(
            list.reveal_center_if_needed(RevealTarget {
                index: 2,
                intra_item_offset: 20,
                extent: 20,
            }),
            90
        );
    }

    #[test]
    fn concrete_engine_is_replaceable() {
        #[derive(Default)]
        struct Stub {
            extent: u64,
        }
        impl VirtualListEngine for Stub {
            fn replace_item_extents(&mut self, sizes: &[u32], _overscan: usize) {
                self.extent = sizes.iter().map(|size| *size as u64).sum();
            }
            fn set_viewport(&mut self, _offset: u64, _viewport_extent: u32) {}
            fn write_window(&self, output: &mut VirtualWindow) {
                output.items.clear();
                output.offset = 0;
                output.viewport_extent = 0;
                output.content_extent = self.extent;
            }
            fn item(&self, _index: usize) -> Option<VirtualItem> {
                None
            }
            fn maximum_offset(&self) -> u64 {
                self.extent
            }
        }

        let list = VirtualList::with_engine(Box::new(Stub::default()), &[2, 3], 0);
        assert_eq!(list.window().content_extent, 5);
    }
}
