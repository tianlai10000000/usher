//! PR Diff adapter for the framework-neutral virtual-list facade.

use std::rc::Rc;

use slint::{Model, ModelRc, VecModel};
use usher_virtual_list::{RevealTarget, VirtualItem, VirtualList, VirtualWindow};

use crate::{
    WorkflowPrDiffLine, WorkflowPrDiffRowMetric, WorkflowPrDiffVirtualRow,
    WorkflowPrDiffVirtualWindow,
};

const DIFF_OVERSCAN_ROWS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    revision: i32,
    code_width_bits: u32,
    cell_width_bits: u32,
    code_line_height_bits: u32,
    hunk_line_height_bits: u32,
    row_count: usize,
}

pub(super) struct PrDiffVirtualListBridge {
    list: VirtualList,
    layout_key: Option<LayoutKey>,
    visible_rows: Rc<VecModel<WorkflowPrDiffVirtualRow>>,
    visible_indices: Vec<usize>,
    window: VirtualWindow,
}

impl Default for PrDiffVirtualListBridge {
    fn default() -> Self {
        Self {
            list: VirtualList::new(&[], DIFF_OVERSCAN_ROWS),
            layout_key: None,
            visible_rows: Rc::new(VecModel::from(Vec::<WorkflowPrDiffVirtualRow>::new())),
            visible_indices: Vec::new(),
            window: VirtualWindow::default(),
        }
    }
}

impl PrDiffVirtualListBridge {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout(
        &mut self,
        revision: i32,
        metrics: &ModelRc<WorkflowPrDiffRowMetric>,
        lines: &ModelRc<WorkflowPrDiffLine>,
        scroll_offset: f32,
        viewport_extent: f32,
        code_width: f32,
        cell_width: f32,
        code_line_height: f32,
        hunk_line_height: f32,
    ) -> WorkflowPrDiffVirtualWindow {
        let key = LayoutKey {
            revision,
            code_width_bits: positive(code_width).to_bits(),
            cell_width_bits: positive(cell_width).to_bits(),
            code_line_height_bits: positive(code_line_height).to_bits(),
            hunk_line_height_bits: positive(hunk_line_height).to_bits(),
            row_count: metrics.row_count(),
        };
        let layout_changed = self.layout_key != Some(key);
        if layout_changed {
            let extents = metrics
                .iter()
                .map(|metric| {
                    row_extent(
                        &metric,
                        code_width,
                        cell_width,
                        code_line_height,
                        hunk_line_height,
                    )
                })
                .collect::<Vec<_>>();
            self.list
                .replace_item_extents(extents.as_slice(), DIFF_OVERSCAN_ROWS);
            self.layout_key = Some(key);
        }

        let viewport_extent = to_u32(viewport_extent).max(1);
        self.list
            .set_viewport(to_u64(scroll_offset), viewport_extent);
        self.list.write_window(&mut self.window);
        Self::sync_visible_rows(
            lines,
            self.window.items.as_slice(),
            layout_changed,
            &self.visible_rows,
            &mut self.visible_indices,
        );

        WorkflowPrDiffVirtualWindow {
            rows: ModelRc::from(self.visible_rows.clone()),
            scroll_offset: self.window.offset.min(f32::MAX as u64) as f32,
            content_height: self.window.content_extent.min(f32::MAX as u64) as f32,
        }
    }

    pub(super) fn reveal_center_if_needed(
        &mut self,
        index: usize,
        intra_item_offset: f32,
        target_extent: f32,
        current_offset: f32,
        viewport_extent: f32,
    ) -> f32 {
        self.list
            .set_viewport(to_u64(current_offset), to_u32(viewport_extent).max(1));
        self.list
            .reveal_center_if_needed(RevealTarget {
                index,
                intra_item_offset: to_u32(intra_item_offset),
                extent: to_u32(target_extent).max(1),
            })
            .min(f32::MAX as u64) as f32
    }

    fn sync_visible_rows(
        lines: &ModelRc<WorkflowPrDiffLine>,
        items: &[VirtualItem],
        layout_changed: bool,
        visible_rows: &Rc<VecModel<WorkflowPrDiffVirtualRow>>,
        visible_indices: &mut Vec<usize>,
    ) {
        if !layout_changed
            && items.len() == visible_indices.len()
            && items
                .iter()
                .zip(visible_indices.iter())
                .all(|(item, index)| item.index == *index)
        {
            return;
        }

        let overlaps = !layout_changed
            && visible_indices
                .first()
                .zip(visible_indices.last())
                .zip(items.first().zip(items.last()))
                .is_some_and(|((old_first, old_last), (new_first, new_last))| {
                    new_first.index <= *old_last && *old_first <= new_last.index
                });
        if !overlaps {
            Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
            return;
        }

        let next_first = items[0].index;
        let next_last = items.last().map_or(next_first, |item| item.index);
        while visible_indices
            .first()
            .is_some_and(|index| *index < next_first)
        {
            visible_indices.remove(0);
            visible_rows.remove(0);
        }
        while visible_indices
            .last()
            .is_some_and(|index| *index > next_last)
        {
            let index = visible_indices.len() - 1;
            visible_indices.pop();
            visible_rows.remove(index);
        }

        while visible_indices
            .first()
            .is_some_and(|index| *index > next_first)
        {
            let index = visible_indices[0] - 1;
            let Some(item) = item_at_index(items, next_first, index) else {
                Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
                return;
            };
            let Some(row) = virtual_row(lines, item) else {
                Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
                return;
            };
            visible_indices.insert(0, index);
            visible_rows.insert(0, row);
        }

        let mut append_from = visible_indices
            .last()
            .copied()
            .map_or(next_first, |index| index.saturating_add(1));
        while append_from <= next_last {
            let Some(item) = item_at_index(items, next_first, append_from) else {
                Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
                return;
            };
            let Some(row) = virtual_row(lines, item) else {
                Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
                return;
            };
            visible_indices.push(append_from);
            visible_rows.push(row);
            append_from = append_from.saturating_add(1);
        }

        if items.len() != visible_indices.len()
            || items
                .iter()
                .zip(visible_indices.iter())
                .any(|(item, index)| item.index != *index)
        {
            Self::replace_visible_rows(lines, items, visible_rows, visible_indices);
        }
    }

    fn replace_visible_rows(
        lines: &ModelRc<WorkflowPrDiffLine>,
        items: &[VirtualItem],
        visible_rows: &Rc<VecModel<WorkflowPrDiffVirtualRow>>,
        visible_indices: &mut Vec<usize>,
    ) {
        let rows = items
            .iter()
            .filter_map(|item| virtual_row(lines, *item))
            .collect::<Vec<_>>();
        visible_indices.clear();
        visible_indices.extend(rows.iter().map(|row| row.index as usize));
        visible_rows.set_vec(rows);
    }
}

fn item_at_index(items: &[VirtualItem], first_index: usize, index: usize) -> Option<VirtualItem> {
    let item = *items.get(index.checked_sub(first_index)?)?;
    (item.index == index).then_some(item)
}

fn virtual_row(
    lines: &ModelRc<WorkflowPrDiffLine>,
    item: VirtualItem,
) -> Option<WorkflowPrDiffVirtualRow> {
    Some(WorkflowPrDiffVirtualRow {
        index: i32::try_from(item.index).ok()?,
        start: item.start.min(f32::MAX as u64) as f32,
        size: item.extent as f32,
        line: lines.row_data(item.index)?,
    })
}

fn row_extent(
    metric: &WorkflowPrDiffRowMetric,
    code_width: f32,
    cell_width: f32,
    code_line_height: f32,
    hunk_line_height: f32,
) -> u32 {
    if metric.kind.as_str() == "hunk" {
        return if metric.double_hunk {
            52
        } else {
            to_u32(hunk_line_height).max(1)
        };
    }
    let content_width = metric.columns.max(0) as f32 * positive(cell_width);
    let visual_lines = (content_width / positive(code_width)).ceil().max(1.0);
    to_u32(visual_lines * positive(code_line_height)).max(1)
}

fn positive(value: f32) -> f32 {
    if value.is_finite() {
        value.max(1.0)
    } else {
        1.0
    }
}

fn to_u32(value: f32) -> u32 {
    if value.is_finite() {
        value.round().clamp(0.0, u32::MAX as f32) as u32
    } else {
        0
    }
}

fn to_u64(value: f32) -> u64 {
    if value.is_finite() {
        value.round().clamp(0.0, u64::MAX as f32) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(kind: &str, columns: i32, double_hunk: bool) -> WorkflowPrDiffRowMetric {
        WorkflowPrDiffRowMetric {
            kind: kind.into(),
            columns,
            double_hunk,
        }
    }

    #[test]
    fn row_extent_matches_diff_wrap_contract() {
        assert_eq!(
            row_extent(&metric("context", 20, false), 100.0, 7.0, 22.0, 28.0),
            44
        );
        assert_eq!(
            row_extent(&metric("hunk", 200, false), 1.0, 7.0, 22.0, 28.0),
            28
        );
        assert_eq!(
            row_extent(&metric("hunk", 0, true), 100.0, 7.0, 22.0, 28.0),
            52
        );
    }
}
