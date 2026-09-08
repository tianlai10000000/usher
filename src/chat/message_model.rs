//! Stable incremental Slint message rows and overlap reconciliation.
use crate::MainWindow;
use crate::chat::session::ChatMode;
use slint::{Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) struct ChatMessageModelBridge {
    pub(super) incremental: bool,
    pub(super) rows_model: Rc<VecModel<crate::ChatMessage>>,
    pub(super) mounted_keys: RefCell<Vec<ChatMessageRowKey>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChatMessageRowKey {
    pub(super) mode: ChatMode,
    pub(super) id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModelMutationCounts {
    pub(super) inserted: usize,
    pub(super) removed: usize,
    pub(super) incremental: bool,
}

impl ChatMessageModelBridge {
    pub(super) fn new(incremental: bool) -> Self {
        Self {
            incremental,
            rows_model: Rc::new(VecModel::default()),
            mounted_keys: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn bind(&self, ui: &MainWindow) {
        if self.incremental {
            ui.set_chat_messages(ModelRc::from(self.rows_model.clone()));
        }
    }

    pub(super) fn replace_rows(
        &self,
        ui: &MainWindow,
        mode: ChatMode,
        next_rows: Vec<crate::ChatMessage>,
        next_top_spacer: f32,
        next_bottom_spacer: f32,
    ) -> ModelMutationCounts {
        let next_keys = next_rows
            .iter()
            .map(|row| ChatMessageRowKey { mode, id: row.id })
            .collect::<Vec<_>>();
        if !self.incremental {
            let removed = ui.get_chat_messages().row_count();
            let inserted = next_rows.len();
            ui.set_chat_messages(next_rows.as_slice().into());
            *self.mounted_keys.borrow_mut() = next_keys;
            return ModelMutationCounts {
                inserted,
                removed,
                incremental: false,
            };
        }

        let old_keys = self.mounted_keys.borrow().clone();
        if old_keys == next_keys {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
            return ModelMutationCounts {
                inserted: 0,
                removed: 0,
                incremental: true,
            };
        }

        let old_top_spacer = ui.get_chat_virtual_top_spacer();
        let old_bottom_spacer = ui.get_chat_virtual_bottom_spacer();
        if next_top_spacer > old_top_spacer {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
        }
        if next_bottom_spacer > old_bottom_spacer {
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
        }

        let overlap = longest_contiguous_overlap(&old_keys, &next_keys);
        let (inserted, removed) = if let Some((old_start, next_start, overlap_len)) = overlap {
            for (index, row) in next_rows.iter().take(next_start).cloned().enumerate() {
                self.rows_model.insert(index, row);
            }
            for row in next_rows.iter().skip(next_start + overlap_len).cloned() {
                self.rows_model.push(row);
            }
            for _ in 0..old_start {
                self.rows_model.remove(next_start);
            }
            let old_suffix = old_keys.len().saturating_sub(old_start + overlap_len);
            for _ in 0..old_suffix {
                self.rows_model.remove(next_start + overlap_len);
            }
            (
                next_rows.len().saturating_sub(overlap_len),
                old_keys.len().saturating_sub(overlap_len),
            )
        } else {
            self.rows_model.set_vec(next_rows);
            (next_keys.len(), old_keys.len())
        };
        if next_top_spacer <= old_top_spacer {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
        }
        if next_bottom_spacer <= old_bottom_spacer {
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
        }
        *self.mounted_keys.borrow_mut() = next_keys;
        ModelMutationCounts {
            inserted,
            removed,
            incremental: overlap.is_some(),
        }
    }

    pub(super) fn clear(&self, ui: &MainWindow) {
        self.mounted_keys.borrow_mut().clear();
        if self.incremental {
            self.rows_model.set_vec(Vec::new());
        } else {
            ui.set_chat_messages([].as_slice().into());
        }
    }
}

pub(super) fn longest_contiguous_overlap<T: PartialEq>(
    old: &[T],
    next: &[T],
) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for (old_start, old_id) in old.iter().enumerate() {
        for (next_start, next_id) in next.iter().enumerate() {
            if old_id != next_id {
                continue;
            }
            let mut len = 0;
            while old_start + len < old.len()
                && next_start + len < next.len()
                && old[old_start + len] == next[next_start + len]
            {
                len += 1;
            }
            if best.is_none_or(|(_, _, best_len)| len > best_len) {
                best = Some((old_start, next_start, len));
            }
        }
    }
    best
}
