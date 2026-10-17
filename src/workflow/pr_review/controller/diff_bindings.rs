//! Diff geometry, selection and Find bindings on the Slint event loop.
use super::super::find::diff_find_intra_item_offset;
use super::super::selection::{
    diff_selection_slice, diff_selection_text, diff_wrap_line_start, resolve_diff_click_range,
    resolve_diff_selection_hit,
};
use super::super::virtual_list::PrDiffVirtualListBridge;
use super::super::{find_bridge, text_geometry};
use super::diagnostics::{
    pr_scroll_diagnostics, write_pr_find_reveal_diagnostic, write_pr_scroll_diagnostic,
};
use crate::pr_review_ui::PrReviewUiExt;
use crate::{
    MainWindow, WorkflowPrDiffGlyphRange, WorkflowPrDiffGlyphRect, WorkflowPrDiffSelectionHit,
    WorkflowPrDiffSelectionRange, WorkflowPrDiffTextSlice, WorkflowPrDiffVirtualWindow,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use usher_search::{SearchDirection, step_match_index};

pub(super) fn bind(ui: &MainWindow) {
    let pr_page = ui.pr_review();
    let diff = pr_page.global::<crate::PrDiffBridge>();
    let diff_virtual_list = Rc::new(RefCell::new(PrDiffVirtualListBridge::default()));
    let layout_virtual_list = diff_virtual_list.clone();
    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_layout_diff_virtual_window(
        move |revision,
              metrics,
              scroll_offset,
              visible_height,
              code_width,
              cell_width,
              code_line_height,
              hunk_line_height| {
            let Some(ui) = ui_weak.upgrade() else {
                return WorkflowPrDiffVirtualWindow::default();
            };
            layout_virtual_list.borrow_mut().layout(
                revision,
                &metrics,
                &ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_lines(),
                scroll_offset,
                visible_height,
                code_width,
                cell_width,
                code_line_height,
                hunk_line_height,
            )
        },
    );

    let scroll_diagnostics_enabled = pr_scroll_diagnostics().is_some();
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .set_workflow_pr_review_scroll_diagnostics_enabled(scroll_diagnostics_enabled);
    diff.on_workflow_pr_review_diagnose_diff_scroll(
        move |kind,
              viewport_y,
              requested_y,
              visible_height,
              viewport_height,
              find_open,
              find_focused,
              hunk_pending,
              find_revision,
              line_revision,
              hunk_attempts| {
            if !scroll_diagnostics_enabled {
                return;
            }
            write_pr_scroll_diagnostic(
                kind.as_str(),
                viewport_y,
                requested_y,
                visible_height,
                viewport_height,
                find_open,
                find_focused,
                hunk_pending,
                find_revision,
                line_revision,
                hunk_attempts,
            );
        },
    );
    if scroll_diagnostics_enabled {
        write_pr_scroll_diagnostic("session", 0.0, 0.0, 0.0, 0.0, false, false, false, 0, 0, 0);
    }

    let ui_weak = ui.as_weak();
    ui.pr_review()
        .global::<crate::PrReviewBridge>()
        .on_workflow_pr_review_copy_diff_selection(
            move |start_line, start_col, end_line, end_col, side| {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                let Some(text) = diff_selection_text(
                    &ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .get_workflow_pr_review_lines(),
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                    side.as_str(),
                ) else {
                    return;
                };
                if text.is_empty() {
                    return;
                }
                // Silent like GitHub's in-diff copy — no toast.
                if let Err(error) =
                    arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text))
                {
                    log::warn!("[Workflow][PR Review] Failed to copy diff selection: {error}");
                }
            },
        );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_selection(
        move |start_line,
              pointer_x,
              pointer_y,
              start_height,
              code_width,
              cell_width,
              line_pitch,
              family,
              font_size,
              side| {
            let Some(ui) = ui_weak.upgrade() else {
                return WorkflowPrDiffSelectionHit::default();
            };
            let lines = ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_lines();
            let scale_factor = ui.window().scale_factor();
            resolve_diff_selection_hit(
                &lines,
                start_line,
                pointer_x,
                pointer_y,
                start_height,
                code_width,
                cell_width,
                line_pitch,
                side.as_str(),
                |line, side, local_x| {
                    let tokens = match side {
                        "left" => &line.left_tokens,
                        "right" => &line.right_tokens,
                        _ => &line.tokens,
                    };
                    text_geometry::hit_test_runs(
                        (0..tokens.row_count()).filter_map(|index| {
                            tokens.row_data(index).map(|token| {
                                (token.text, if token.bold { 600 } else { 400 }, token.italic)
                            })
                        }),
                        local_x,
                        family.as_str(),
                        font_size,
                        scale_factor,
                    )
                },
                |line, side, local_x, local_y, width, line_pitch| {
                    let content = match side {
                        "left" => line.left_content.as_str(),
                        "right" => line.right_content.as_str(),
                        _ => line.content.as_str(),
                    };
                    text_geometry::hit_test_wrapped(
                        text_geometry::TextGeometryOptions {
                            text: content,
                            rendered_width: width,
                            line_pitch,
                            family: family.as_str(),
                            font_size,
                            weight: 400,
                            italic: false,
                            scale_factor,
                        },
                        local_x,
                        local_y,
                    )
                },
            )
        },
    );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_token_hit(
        move |text, pointer_x, token_width, family, font_size, bold, italic| {
            let scale_factor = ui_weak
                .upgrade()
                .map_or(1.0, |ui| ui.window().scale_factor());
            text_geometry::hit_test(
                text_geometry::TextGeometryOptions {
                    text: text.as_str(),
                    rendered_width: token_width,
                    line_pitch: 0.0,
                    family: family.as_str(),
                    font_size,
                    weight: if bold { 600 } else { 400 },
                    italic,
                    scale_factor,
                },
                pointer_x,
            )
        },
    );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_token_range(
        move |text, start, end, token_width, family, font_size, bold, italic| {
            let scale_factor = ui_weak
                .upgrade()
                .map_or(1.0, |ui| ui.window().scale_factor());
            let geometry = text_geometry::range_geometry(
                text_geometry::TextGeometryOptions {
                    text: text.as_str(),
                    rendered_width: token_width,
                    line_pitch: 0.0,
                    family: family.as_str(),
                    font_size,
                    weight: if bold { 600 } else { 400 },
                    italic,
                    scale_factor,
                },
                start,
                end,
            );
            WorkflowPrDiffGlyphRange {
                x: geometry.x,
                width: geometry.width,
            }
        },
    );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_wrapped_range(
        move |text, start, end, width, line_pitch, family, font_size| {
            let scale_factor = ui_weak
                .upgrade()
                .map_or(1.0, |ui| ui.window().scale_factor());
            ModelRc::new(VecModel::from(
                text_geometry::wrapped_range_geometry(
                    text_geometry::TextGeometryOptions {
                        text: text.as_str(),
                        rendered_width: width,
                        line_pitch,
                        family: family.as_str(),
                        font_size,
                        weight: 400,
                        italic: false,
                        scale_factor,
                    },
                    start,
                    end,
                )
                .into_iter()
                .map(|geometry| WorkflowPrDiffGlyphRect {
                    x: geometry.x,
                    y: geometry.y,
                    width: geometry.width,
                    height: geometry.height,
                })
                .collect::<Vec<_>>(),
            ))
        },
    );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_selection_slice(
        move |line, side, start_col, end_col| {
            let Some(ui) = ui_weak.upgrade() else {
                return WorkflowPrDiffTextSlice::default();
            };
            diff_selection_slice(
                &ui.pr_review()
                    .global::<crate::PrReviewBridge>()
                    .get_workflow_pr_review_lines(),
                line,
                side.as_str(),
                start_col,
                end_col,
            )
        },
    );

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_wrap_line_start(move |line, side, caret, max_cols| {
        let Some(ui) = ui_weak.upgrade() else {
            return 0;
        };
        diff_wrap_line_start(
            &ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_lines(),
            line,
            side.as_str(),
            caret,
            max_cols,
        )
    });

    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_click_range(move |line, column, side, mode| {
        let Some(ui) = ui_weak.upgrade() else {
            return WorkflowPrDiffSelectionRange::default();
        };
        resolve_diff_click_range(
            &ui.pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_lines(),
            line,
            column,
            side.as_str(),
            mode,
        )
    });

    find_bridge::bind(ui);

    diff.on_workflow_pr_review_step_diff_find_index(move |current, count, backward| {
        step_match_index(
            (current >= 0).then_some(current as usize),
            count.max(0) as usize,
            if backward {
                SearchDirection::Backward
            } else {
                SearchDirection::Forward
            },
        )
        .map_or(-1, |index| index as i32)
    });

    let reveal_virtual_list = diff_virtual_list.clone();
    let ui_weak = ui.as_weak();
    diff.on_workflow_pr_review_resolve_diff_find_viewport_y(
        move |line,
              start_char,
              code_width,
              cell_width,
              side,
              current_viewport_y,
              visible_height,
              content_height| {
            let Some(ui) = ui_weak.upgrade() else {
                return 0.0;
            };
            let lines = ui
                .pr_review()
                .global::<crate::PrReviewBridge>()
                .get_workflow_pr_review_lines();
            let intra_item_offset = diff_find_intra_item_offset(
                &lines,
                line,
                start_char,
                code_width,
                cell_width,
                side.as_str(),
            );
            let target_viewport_y = -reveal_virtual_list.borrow_mut().reveal_center_if_needed(
                line.max(0) as usize,
                intra_item_offset,
                22.0,
                -current_viewport_y,
                visible_height,
            );
            if scroll_diagnostics_enabled {
                write_pr_find_reveal_diagnostic(
                    line,
                    start_char,
                    lines.row_count(),
                    code_width,
                    cell_width,
                    side.as_str(),
                    current_viewport_y,
                    target_viewport_y,
                    visible_height,
                    content_height,
                );
            }
            target_viewport_y
        },
    );

    let reveal_location_virtual_list = diff_virtual_list.clone();
    diff.on_workflow_pr_review_resolve_location_viewport_y(
        move |line, current_viewport_y, visible_height| {
            -reveal_location_virtual_list
                .borrow_mut()
                .reveal_row_center_if_needed(
                    line.max(0) as usize,
                    -current_viewport_y,
                    visible_height,
                )
        },
    );
}
