use super::request;
use crate::ai::agent::CompletionMessage;
use crate::app_runtime::shared_runtime;
use crate::chat::chat::handle_api_error;
use crate::chat::persistence::{StoredMessageRole, StoredMessageStatus};
use crate::chat::renderer::{clear_render_cache, record_model_commit, render_for_ui};
use crate::chat::session::{ChatMode, ChatRequestState, ChatSessions};
use crate::chat::virtual_layout::{
    VirtualRenderSignature, VirtualWindowPolicy, clear_virtual_layout_cache,
    compute_history_window, compute_spacers_for_fixed_window, current_virtual_document_height_px,
    get_adaptive_row_height_px, smooth_adaptive_row_height_px,
};
use crate::{MainWindow, Route, ToastManager, ToastOptions};
use slint::{ComponentHandle, Global, Model};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const LOADING_PHASE_INTERVAL_MS: u64 = 700; // 低频点点动画，降低发送中主线程更新压力
const LOADING_PHASE_ANIMATION_ENABLED: bool = false;
const STICK_TO_BOTTOM_THRESHOLD_PX: f32 = 96.0;

/// Chat sessions bootstrap on a blocking worker; `None` marks the loading
/// window between startup and the event-loop backfill.
type SharedChatSessions = Arc<Mutex<Option<ChatSessions>>>;

#[derive(Default)]
struct ChatDrafts {
    ai: String,
    rag: String,
}

impl ChatDrafts {
    fn get(&self, mode: ChatMode) -> &str {
        match mode {
            ChatMode::Ai => self.ai.as_str(),
            ChatMode::Rag => self.rag.as_str(),
        }
    }

    fn set(&mut self, mode: ChatMode, text: String) {
        match mode {
            ChatMode::Ai => self.ai = text,
            ChatMode::Rag => self.rag = text,
        }
    }
}

#[cfg(test)]
use super::viewport_policy::VisibleAnchor;
use super::viewport_policy::{
    ANCHOR_SETTLE_INTERVAL_MS, AnchorPurpose, AnchorSettle, AnchorStep, LAYOUT_SETTLE_DELAY_MS,
    MAX_NEW_ROWS_PER_SCROLL_COMMIT, MOUNTED_EDGE_GUARD_PX, MessageGeometry,
    compute_wrap_width_for_chat_width, estimated_window_covers_viewport,
    limit_incremental_virtual_shift, scroll_update_bucket_step, select_visible_anchor,
    shifted_prepend_window, should_shift_near_estimated_edge, should_shift_near_mounted_edge,
    should_shift_virtual_window, update_auto_load_armed, virtual_scroll_bucket,
};

fn should_shift_near_virtual_edge(
    ui: &MainWindow,
    geometries: &HashMap<i32, MessageGeometry>,
    scroll_offset_px: f32,
    viewport_height_px: f32,
    moving_down: bool,
    has_more_in_direction: bool,
) -> (bool, bool) {
    if !crate::chat::performance::virtualization_enabled() {
        return (false, false);
    }
    // Coverage is not directional: a prepend/scrollbar jump can put the viewport
    // on either spacer even when the last applied scroll bucket suggests the
    // opposite direction.
    let uncovered = virtual_blank_exposure_px(scroll_offset_px, viewport_height_px) > 1.0;
    let near_mounted = uncovered
        || should_shift_near_mounted_edge(
            geometries,
            scroll_offset_px,
            viewport_height_px,
            moving_down,
            has_more_in_direction,
        );
    let near_estimated = should_shift_near_estimated_edge(
        ui.get_chat_virtual_top_spacer(),
        ui.get_chat_virtual_bottom_spacer(),
        current_virtual_document_height_px(),
        scroll_offset_px,
        viewport_height_px,
        moving_down,
        has_more_in_direction,
    );
    (near_mounted, near_mounted || near_estimated)
}

thread_local! {
    static LAST_RENDER_SIGNATURE: RefCell<Option<VirtualRenderSignature>> = const { RefCell::new(None) };
    static MESSAGE_GEOMETRIES: RefCell<HashMap<i32, MessageGeometry>> = RefCell::new(HashMap::new());
    static PENDING_HISTORY_ANCHOR: RefCell<Option<AnchorSettle>> = const { RefCell::new(None) };
    static ACTIVE_VIRTUAL_WRAP_WIDTH_PX: Cell<i32> = const { Cell::new(0) };
    static MODEL_MUTATION_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    static CHAT_MESSAGE_MODEL: RefCell<Option<Rc<ChatMessageModelBridge>>> = const { RefCell::new(None) };
    static CHAT_SESSION: RefCell<Option<SharedChatSessions>> = const { RefCell::new(None) };
    static ANCHOR_GENERATION: Cell<u64> = const { Cell::new(0) };
    static ANCHOR_SETTLE_REVISION: Cell<u64> = const { Cell::new(0) };
    static USER_SCROLL_REVISION: Cell<u64> = const { Cell::new(0) };
    static GEOMETRY_REVISION: Cell<u64> = const { Cell::new(0) };
    static ANCHOR_SCROLL_WRITE_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    static ANCHOR_SCROLL_WRITE_COUNT: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn sync_language(ui: &MainWindow) {
    request::sync_agent_identity(ui);
    clear_render_cache();
    clear_virtual_layout_cache();
    LAST_RENDER_SIGNATURE.with(|last| *last.borrow_mut() = None);
    if ui.get_current_route() != Route::Chat {
        return;
    }
    let Some(session) = CHAT_SESSION.with(|slot| slot.borrow().clone()) else {
        return;
    };
    CHAT_MESSAGE_MODEL.with(|slot| {
        if let Some(bridge) = slot.borrow().as_ref() {
            bridge.clear(ui);
        }
    });
    apply_chat_state_to_ui(
        ui,
        &session,
        chat_mode_from_ui(ui),
        ui.get_chat_is_sending(),
        Some(active_wrap_width_px(ui)),
        false,
    );
}

fn schedule_anchor_compensation_after(
    ui_weak: slint::Weak<MainWindow>,
    delay: std::time::Duration,
) {
    let Some(generation) = PENDING_HISTORY_ANCHOR
        .with(|pending| pending.borrow().as_ref().map(|anchor| anchor.generation))
    else {
        return;
    };
    let revision = ANCHOR_SETTLE_REVISION.with(|latest| {
        let revision = latest.get().wrapping_add(1);
        latest.set(revision);
        revision
    });
    slint::Timer::single_shot(delay, move || {
        if ANCHOR_SETTLE_REVISION.with(Cell::get) != revision {
            return;
        }
        let generation_is_current = PENDING_HISTORY_ANCHOR.with(|pending| {
            pending
                .borrow()
                .as_ref()
                .is_some_and(|anchor| anchor.generation == generation)
        });
        if !generation_is_current {
            return;
        }
        if MODEL_MUTATION_IN_PROGRESS.with(Cell::get) {
            schedule_anchor_compensation_after(
                ui_weak,
                std::time::Duration::from_millis(ANCHOR_SETTLE_INTERVAL_MS),
            );
            return;
        }

        let Some(ui) = ui_weak.upgrade() else {
            clear_pending_anchor_if_generation(generation);
            return;
        };
        if ui.get_current_route() != Route::Chat || ui.get_chat_scrollbar_dragging() {
            clear_pending_anchor_if_generation(generation);
            return;
        }

        let (message_id, expected_mode) = PENDING_HISTORY_ANCHOR.with(|pending| {
            let pending = pending.borrow();
            let anchor = pending.as_ref().expect("anchor generation checked above");
            (anchor.message_id, anchor.mode)
        });
        if chat_mode_from_ui(&ui) != expected_mode {
            clear_pending_anchor_if_generation(generation);
            return;
        }
        let observation =
            MESSAGE_GEOMETRIES.with(|geometries| geometries.borrow().get(&message_id).copied());
        let current_scroll_offset = (-ui.get_chat_scroll_position()).max(0.0);
        let input_revision = USER_SCROLL_REVISION.with(Cell::get);
        let step = PENDING_HISTORY_ANCHOR.with(|pending| {
            let mut pending = pending.borrow_mut();
            let anchor = pending.as_mut().expect("anchor generation checked above");
            anchor.step(
                expected_mode,
                input_revision,
                current_scroll_offset,
                observation,
            )
        });

        match step {
            AnchorStep::Write(target_offset) => {
                ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(|active| active.set(true));
                ui.set_chat_scroll_position(-target_offset);
                ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(|active| active.set(false));
                ANCHOR_SCROLL_WRITE_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
                schedule_anchor_compensation_after(
                    ui.as_weak(),
                    std::time::Duration::from_millis(ANCHOR_SETTLE_INTERVAL_MS),
                );
            }
            AnchorStep::Wait => schedule_anchor_compensation_after(
                ui.as_weak(),
                std::time::Duration::from_millis(ANCHOR_SETTLE_INTERVAL_MS),
            ),
            AnchorStep::Complete | AnchorStep::Cancel => {
                clear_pending_anchor_if_generation(generation);
            }
        }
    });
}

fn schedule_anchor_compensation(ui_weak: slint::Weak<MainWindow>) {
    schedule_anchor_compensation_after(ui_weak, std::time::Duration::ZERO);
}

use super::message_model::ChatMessageModelBridge;
#[cfg(test)]
use super::message_model::{ChatMessageRowKey, longest_contiguous_overlap};

fn update_virtual_spacers(ui: &MainWindow, top_spacer: f32, bottom_spacer: f32) {
    let was_active = MODEL_MUTATION_IN_PROGRESS.with(|active| active.replace(true));
    let old_top = ui.get_chat_virtual_top_spacer();
    let old_bottom = ui.get_chat_virtual_bottom_spacer();
    if top_spacer > old_top {
        ui.set_chat_virtual_top_spacer(top_spacer);
    }
    if bottom_spacer > old_bottom {
        ui.set_chat_virtual_bottom_spacer(bottom_spacer);
    }
    if top_spacer <= old_top {
        ui.set_chat_virtual_top_spacer(top_spacer);
    }
    if bottom_spacer <= old_bottom {
        ui.set_chat_virtual_bottom_spacer(bottom_spacer);
    }
    MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(was_active));
}

fn clear_history_anchor_state() {
    MESSAGE_GEOMETRIES.with(|geometries| geometries.borrow_mut().clear());
    clear_pending_anchor();
}

fn clear_pending_anchor() {
    ANCHOR_GENERATION.with(|generation| generation.set(generation.get().wrapping_add(1)));
    PENDING_HISTORY_ANCHOR.with(|anchor| *anchor.borrow_mut() = None);
}

fn clear_pending_anchor_if_generation(expected_generation: u64) {
    let should_clear = PENDING_HISTORY_ANCHOR.with(|pending| {
        pending
            .borrow()
            .as_ref()
            .is_some_and(|anchor| anchor.generation == expected_generation)
    });
    if should_clear {
        clear_pending_anchor();
    }
}

fn has_pending_anchor() -> bool {
    PENDING_HISTORY_ANCHOR.with(|anchor| anchor.borrow().is_some())
}

fn virtual_window_has_more(moving_down: bool) -> bool {
    LAST_RENDER_SIGNATURE.with(|last| {
        last.borrow().is_some_and(|signature| {
            if moving_down {
                signature.end < signature.total_len
            } else {
                signature.start > 0
            }
        })
    })
}

pub(crate) fn virtual_blank_exposure_px(scroll_offset: f32, viewport_height: f32) -> f32 {
    let Some(signature) = LAST_RENDER_SIGNATURE.with(|last| *last.borrow()) else {
        return 0.0;
    };
    MESSAGE_GEOMETRIES.with(|geometries| {
        let geometries = geometries.borrow();
        if geometries.is_empty() {
            return 0.0;
        }
        let mounted_top = geometries
            .values()
            .map(|geometry| geometry.y)
            .fold(f32::INFINITY, f32::min);
        let mounted_bottom = geometries
            .values()
            .map(|geometry| geometry.y + geometry.height)
            .fold(f32::NEG_INFINITY, f32::max);
        let viewport_bottom = scroll_offset + viewport_height;
        let top_blank = if signature.start > 0 {
            (mounted_top - scroll_offset).clamp(0.0, viewport_height)
        } else {
            0.0
        };
        let bottom_blank = if signature.end < signature.total_len {
            (viewport_bottom - mounted_bottom).clamp(0.0, viewport_height)
        } else {
            0.0
        };
        top_blank.max(bottom_blank)
    })
}

fn capture_visible_anchor(ui: &MainWindow, purpose: AnchorPurpose, max_correction_px: Option<f32>) {
    if ui.get_chat_scrollbar_dragging() {
        clear_pending_anchor();
        return;
    }
    let model = ui.get_chat_messages();
    let mounted_ids = (0..model.row_count())
        .filter_map(|index| model.row_data(index).map(|message| message.id))
        .collect::<std::collections::HashSet<_>>();
    let scroll_offset = (-ui.get_chat_scroll_position()).max(0.0);
    let viewport_height = ui.get_chat_message_viewport_height().max(1.0);
    let anchor = MESSAGE_GEOMETRIES.with(|geometries| {
        let mut geometries = geometries.borrow_mut();
        geometries.retain(|message_id, _| mounted_ids.contains(message_id));
        select_visible_anchor(&geometries, scroll_offset, viewport_height)
    });
    let generation = ANCHOR_GENERATION.with(|generation| {
        let next = generation.get().wrapping_add(1);
        generation.set(next);
        next
    });
    let input_revision = USER_SCROLL_REVISION.with(Cell::get);
    let mode = chat_mode_from_ui(ui);
    PENDING_HISTORY_ANCHOR.with(|pending| {
        *pending.borrow_mut() = anchor.map(|anchor| {
            AnchorSettle::new(
                generation,
                mode,
                purpose,
                anchor,
                input_revision,
                max_correction_px,
            )
        });
    });
}

pub(crate) fn anchor_scroll_write_count() -> u64 {
    ANCHOR_SCROLL_WRITE_COUNT.with(Cell::get)
}

fn capture_visible_history_anchor(ui: &MainWindow) {
    capture_visible_anchor(ui, AnchorPurpose::HistoryPrepend, None);
}

fn compute_wrap_width_px(ui: &MainWindow) -> i32 {
    let window_width = ui.get_chat_layout_window_width();
    let nav_width = if ui.get_navigation_drawer_expanded() {
        240.0
    } else {
        0.0
    };
    let chat_content_width = (window_width - nav_width - 24.0).max(320.0);
    compute_wrap_width_for_chat_width(chat_content_width)
}

fn active_wrap_width_px(ui: &MainWindow) -> i32 {
    ACTIVE_VIRTUAL_WRAP_WIDTH_PX.with(|width| {
        let active = width.get();
        if active > 0 {
            active
        } else {
            compute_wrap_width_px(ui)
        }
    })
}

fn apply_chat_state_to_ui(
    ui: &MainWindow,
    session: &SharedChatSessions,
    mode: ChatMode,
    include_loading_placeholder: bool,
    wrap_width_override_px: Option<i32>,
    limit_scroll_insertions: bool,
) {
    apply_chat_state_to_ui_with_policy(
        ui,
        session,
        mode,
        include_loading_placeholder,
        wrap_width_override_px,
        limit_scroll_insertions,
        None,
    );
}

fn apply_chat_state_to_ui_for_scrollbar_drag(
    ui: &MainWindow,
    session: &SharedChatSessions,
    mode: ChatMode,
    include_loading_placeholder: bool,
    wrap_width_override_px: Option<i32>,
    moving_down: bool,
) {
    let window_policy = if crate::chat::performance::variant().is_optimized() {
        VirtualWindowPolicy::direct_scrollbar_drag(moving_down)
    } else {
        VirtualWindowPolicy::baseline()
    };
    apply_chat_state_to_ui_with_policy(
        ui,
        session,
        mode,
        include_loading_placeholder,
        wrap_width_override_px,
        false,
        Some(window_policy),
    );
}

fn apply_chat_state_to_ui_with_policy(
    ui: &MainWindow,
    session: &SharedChatSessions,
    mode: ChatMode,
    include_loading_placeholder: bool,
    wrap_width_override_px: Option<i32>,
    limit_scroll_insertions: bool,
    window_policy_override: Option<VirtualWindowPolicy>,
) {
    let (
        messages,
        can_load_more,
        top_spacer_px,
        bottom_spacer_px,
        document_height_px,
        render_signature,
    ) = {
        let guard = match session.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("[Chat::Session] Lock poisoned: {}", e);
                return;
            }
        };
        let Some(guard) = guard.as_ref() else {
            // Bootstrap has not backfilled the sessions yet; keep the
            // loading/empty state until the completion handler remounts.
            return;
        };

        let all_messages = guard.messages_for(mode);
        let total = all_messages.len();

        if total == 0 {
            (
                Vec::new(),
                guard.can_load_more_for(mode),
                0.0f32,
                0.0f32,
                0.0f32,
                Some(VirtualRenderSignature {
                    total_len: 0,
                    start: 0,
                    end: 0,
                    include_loading_placeholder,
                }),
            )
        } else {
            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let viewport_height_px = ui.get_chat_message_viewport_height().max(1.0);
            let history_inset_px = 24.0
                + if guard.can_load_more_for(mode)
                    || ui.get_chat_loading_history()
                    || ui.get_chat_history_load_failed()
                {
                    64.0
                } else {
                    0.0
                };
            let message_offset_px = (scroll_offset_px - history_inset_px).max(0.0);
            let wrap_width_px = wrap_width_override_px.unwrap_or_else(|| compute_wrap_width_px(ui));
            ACTIVE_VIRTUAL_WRAP_WIDTH_PX.with(|width| width.set(wrap_width_px));
            let virtualization_enabled = crate::chat::performance::virtualization_enabled();
            let row_estimate_px = get_adaptive_row_height_px();
            let window_policy = window_policy_override.unwrap_or_else(|| {
                if crate::chat::performance::variant().is_optimized() {
                    VirtualWindowPolicy::optimized()
                } else {
                    VirtualWindowPolicy::baseline()
                }
            });
            let (mut start, mut end, mut top_px, mut bottom_px, mut sampled_avg_height_px) =
                compute_history_window(
                    all_messages,
                    virtualization_enabled,
                    message_offset_px,
                    viewport_height_px,
                    wrap_width_px,
                    row_estimate_px,
                    window_policy,
                );
            let pending_history_anchor = PENDING_HISTORY_ANCHOR.with(|pending| {
                pending.borrow().as_ref().and_then(|anchor| {
                    (anchor.mode == mode && anchor.purpose == AnchorPurpose::HistoryPrepend)
                        .then_some(anchor.message_id)
                })
            });
            if virtualization_enabled
                && let Some(anchor_id) = pending_history_anchor
                && let Some(previous) = LAST_RENDER_SIGNATURE.with(|last| *last.borrow())
                && previous.include_loading_placeholder == include_loading_placeholder
                && let Some(anchor_index) = all_messages
                    .iter()
                    .position(|message| message.id as i32 == anchor_id)
                && let Some((preserved_start, preserved_end)) =
                    shifted_prepend_window(previous, total, anchor_index)
                && let Some((preserved_top, preserved_bottom, preserved_average)) =
                    compute_spacers_for_fixed_window(
                        all_messages,
                        preserved_start,
                        preserved_end,
                        wrap_width_px,
                        true,
                    )
            {
                // Keep the reading anchor and its newly loaded predecessors
                // mounted together, including pages containing very tall rows.
                start = preserved_start;
                end = preserved_end;
                top_px = preserved_top;
                bottom_px = preserved_bottom;
                sampled_avg_height_px = preserved_average;
            }
            if limit_scroll_insertions
                && virtualization_enabled
                && crate::chat::performance::variant().is_optimized()
                && let Some(previous) = LAST_RENDER_SIGNATURE.with(|last| *last.borrow())
                && previous.total_len == total
                && previous.include_loading_placeholder == include_loading_placeholder
            {
                let limited = limit_incremental_virtual_shift(
                    previous,
                    start,
                    end,
                    MAX_NEW_ROWS_PER_SCROLL_COMMIT,
                );
                if limited != (start, end)
                    && let Some((limited_top, limited_bottom, _)) = compute_spacers_for_fixed_window(
                        all_messages,
                        limited.0,
                        limited.1,
                        wrap_width_px,
                        true,
                    )
                    && estimated_window_covers_viewport(
                        limited_top,
                        limited_bottom,
                        current_virtual_document_height_px(),
                        message_offset_px,
                        viewport_height_px,
                    )
                {
                    start = limited.0;
                    end = limited.1;
                    top_px = limited_top;
                    bottom_px = limited_bottom;
                }
            }
            smooth_adaptive_row_height_px(sampled_avg_height_px);
            let document_height_px = if virtualization_enabled {
                current_virtual_document_height_px()
            } else {
                0.0
            };
            (
                all_messages[start..end].to_vec(),
                guard.can_load_more_for(mode),
                top_px,
                bottom_px,
                document_height_px,
                Some(VirtualRenderSignature {
                    total_len: total,
                    start,
                    end,
                    include_loading_placeholder,
                }),
            )
        }
    };

    ui.set_chat_can_load_more_history(can_load_more);
    let should_render = LAST_RENDER_SIGNATURE.with(|last| {
        let mut slot = last.borrow_mut();
        let same = *slot == render_signature;
        if !same {
            *slot = render_signature;
        }
        !same
    });
    if should_render {
        let mounted_ids = messages
            .iter()
            .map(|message| message.id as i32)
            .collect::<std::collections::HashSet<_>>();
        let rendered = render_for_ui(&messages, include_loading_placeholder);
        let mutation_started = Instant::now();
        MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(true));
        let counts = CHAT_MESSAGE_MODEL.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("Chat message model must be initialized on the event loop")
                .replace_rows(ui, mode, rendered, top_spacer_px, bottom_spacer_px)
        });
        MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(false));
        MESSAGE_GEOMETRIES.with(|geometries| {
            geometries
                .borrow_mut()
                .retain(|message_id, _| mounted_ids.contains(message_id));
        });
        record_model_commit(
            mutation_started.elapsed(),
            counts.inserted,
            counts.removed,
            counts.incremental,
        );
        log::debug!(
            "[Chat::Model] committed mode={mode:?} rows={} inserted={} removed={} incremental={}",
            messages.len(),
            counts.inserted,
            counts.removed,
            counts.incremental
        );
        schedule_anchor_compensation(ui.as_weak());
    } else {
        update_virtual_spacers(ui, top_spacer_px, bottom_spacer_px);
        schedule_anchor_compensation(ui.as_weak());
    }
    ui.set_chat_virtual_document_height(document_height_px);
}

fn clear_chat_ui_state(ui: &MainWindow) {
    CHAT_MESSAGE_MODEL.with(|slot| {
        if let Some(model_bridge) = slot.borrow().as_ref() {
            model_bridge.clear(ui);
        } else {
            ui.set_chat_messages([].as_slice().into());
        }
    });
    ui.set_chat_can_load_more_history(false);
    ui.set_chat_loading_history(false);
    ui.set_chat_history_load_failed(false);
    ui.set_chat_history_restoring(false);
    ui.set_chat_virtual_top_spacer(0.0);
    ui.set_chat_virtual_bottom_spacer(0.0);
    ui.set_chat_virtual_document_height(0.0);
    ui.set_chat_loading_phase(0);
    ui.set_chat_request_elapsed_seconds(0);
}

fn schedule_chat_ui_mount(
    ui_weak: slint::Weak<MainWindow>,
    session: SharedChatSessions,
    mode: ChatMode,
    revision: u64,
    current_revision: Rc<Cell<u64>>,
    scroll_to_latest: bool,
) {
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if ui.get_current_route() != Route::Chat
            || !chat_ui_mount_is_current(
                mode,
                revision,
                chat_mode_from_ui(&ui),
                current_revision.get(),
            )
        {
            return;
        }
        let session_ready = session.lock().map(|guard| guard.is_some()).unwrap_or(false);
        if !session_ready {
            // Keep the restoring indicator; the bootstrap completion handler
            // remounts through the route callback once history is available.
            return;
        }

        // The conditional Chat component is instantiated during the first layout
        // pass after the route property changes. Re-applying the model on the next
        // event-loop tick ensures its repeaters and ScrollView already exist.
        LAST_RENDER_SIGNATURE.with(|last| {
            *last.borrow_mut() = None;
        });
        let window_policy = scroll_to_latest.then(|| {
            let policy = if crate::chat::performance::variant().is_optimized() {
                VirtualWindowPolicy::optimized()
            } else {
                VirtualWindowPolicy::baseline()
            };
            policy.for_latest_messages()
        });
        apply_chat_state_to_ui_with_policy(
            &ui,
            &session,
            mode,
            ui.get_chat_is_sending(),
            Some(active_wrap_width_px(&ui)),
            false,
            window_policy,
        );
        ui.set_chat_history_restoring(false);
        if scroll_to_latest {
            scroll_chat_to_bottom(&ui);
        }
    });
}

fn begin_chat_ui_mount(ui: &MainWindow, session: &SharedChatSessions, mode: ChatMode) {
    let has_history = session
        .lock()
        .map(|guard| {
            guard
                .as_ref()
                // While bootstrap is still loading, show the restoring state.
                .is_none_or(|sessions| !sessions.messages_for(mode).is_empty())
        })
        .unwrap_or_else(|error| {
            log::error!("[Chat::Session] Lock poisoned while mounting Chat: {error}");
            false
        });
    ui.set_chat_history_restoring(has_history);
}

fn next_chat_ui_mount_revision(current_revision: &Cell<u64>) -> u64 {
    let revision = current_revision.get().wrapping_add(1);
    current_revision.set(revision);
    revision
}

fn chat_ui_mount_is_current(
    expected_mode: ChatMode,
    expected_revision: u64,
    current_mode: ChatMode,
    current_revision: u64,
) -> bool {
    expected_mode == current_mode && expected_revision == current_revision
}

fn schedule_loading_phase_tick(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    slint::Timer::single_shot(
        std::time::Duration::from_millis(LOADING_PHASE_INTERVAL_MS),
        move || {
            if !loading_phase_active.load(Ordering::Relaxed) {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                if ui.get_chat_is_sending() && ui.get_current_route() == Route::Chat {
                    let next = (ui.get_chat_loading_phase() + 1) % 3;
                    ui.set_chat_loading_phase(next);
                }
                schedule_loading_phase_tick(ui_weak.clone(), loading_phase_active.clone());
            } else {
                loading_phase_active.store(false, Ordering::Relaxed);
            }
        },
    );
}

fn start_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    if !LOADING_PHASE_ANIMATION_ENABLED {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_chat_loading_phase(0);
        }
        loading_phase_active.store(false, Ordering::Relaxed);
        return;
    }
    if loading_phase_active.load(Ordering::Relaxed) {
        return;
    }
    loading_phase_active.store(true, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
    schedule_loading_phase_tick(ui_weak, loading_phase_active);
}

fn stop_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    loading_phase_active.store(false, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
}

fn scroll_chat_to_bottom(ui: &MainWindow) {
    clear_pending_anchor();
    ui.set_chat_scroll_to_bottom_revision(ui.get_chat_scroll_to_bottom_revision().wrapping_add(1));
}

fn show_chat_storage_error(ui: &MainWindow) {
    ToastManager::get(ui).invoke_error(ToastOptions {
        message: crate::i18n::tr("chat.storage_failed").as_ref().into(),
        duration: 3_500,
        position: "middle".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: true,
        loading_type: "".into(),
        width: "auto".into(),
    });
}

fn chat_mode_from_ui(ui: &MainWindow) -> ChatMode {
    if ui.get_chat_ai_mode() {
        ChatMode::Ai
    } else {
        ChatMode::Rag
    }
}

fn ai_completion_messages(
    history: &[crate::chat::persistence::StoredChatMessage],
) -> Vec<CompletionMessage> {
    history
        .iter()
        .map(|message| match message.role {
            StoredMessageRole::User => CompletionMessage::user(message.content.clone()),
            StoredMessageRole::Assistant => CompletionMessage::assistant(message.content.clone()),
        })
        .collect()
}

/// 设置聊天交互逻辑和事件回调
pub fn setup_interactive(ui: &MainWindow) {
    setup_interactive_with_sessions(ui, None);
}

fn setup_interactive_with_sessions(ui: &MainWindow, initial: Option<ChatSessions>) {
    log::info!("[Chat::Interactive] Setting up interactive callbacks...");
    let runtime = shared_runtime();

    // Sessions bootstrap on a blocking worker and are backfilled on the event
    // loop; the UI starts from the empty/loading state until then.
    let bootstrap = initial.is_none();
    let session: SharedChatSessions = Arc::new(Mutex::new(initial));
    CHAT_SESSION.with(|slot| *slot.borrow_mut() = Some(session.clone()));
    let request_state = Arc::new(Mutex::new(ChatRequestState::default()));
    let drafts = Arc::new(Mutex::new(ChatDrafts::default()));
    ui.set_chat_ai_mode(true);
    request::sync_agent_identity(&ui);
    let performance_variant = crate::chat::performance::variant();
    ui.set_chat_performance_optimized(performance_variant.is_optimized());
    log::info!(
        "[Chat::Performance] variant={}",
        performance_variant.label()
    );
    let model_bridge = Rc::new(ChatMessageModelBridge::new(
        performance_variant.is_optimized(),
    ));
    model_bridge.bind(ui);
    CHAT_MESSAGE_MODEL.with(|slot| *slot.borrow_mut() = Some(model_bridge));

    let ui_weak_bootstrap = ui.as_weak();
    let session_bootstrap = session.clone();
    if bootstrap {
        runtime.spawn(async move {
            let bootstrapped = tokio::task::spawn_blocking(|| {
                ChatSessions::bootstrap_default().map(|mut sessions| {
                    sessions.install_performance_fixture();
                    sessions
                })
            })
            .await;

            let _ = slint::invoke_from_event_loop(move || {
                let sessions = match bootstrapped {
                    Ok(Ok(sessions)) => sessions,
                    Ok(Err(e)) => {
                        log::error!("[Chat::Storage] Failed to initialize chat storage: {}", e);
                        if let Some(ui) = ui_weak_bootstrap.upgrade() {
                            ui.set_chat_history_restoring(false);
                        }
                        return;
                    }
                    Err(e) => {
                        log::error!("[Chat::Storage] Bootstrap task join failed: {}", e);
                        if let Some(ui) = ui_weak_bootstrap.upgrade() {
                            ui.set_chat_history_restoring(false);
                        }
                        return;
                    }
                };
                sessions.log_storage_paths();
                log::info!(
                    "[Chat::History] bootstrap loaded_messages={} can_load_more={}",
                    sessions.loaded_messages().len(),
                    sessions.can_load_more()
                );
                if let Ok(mut slot) = session_bootstrap.lock() {
                    *slot = Some(sessions);
                }
                let Some(ui) = ui_weak_bootstrap.upgrade() else {
                    return;
                };
                if ui.get_current_route() == Route::Chat {
                    // Remount through the route callback so revision tracking and
                    // first-entry scrolling behave exactly like a fresh entry.
                    ui.invoke_chat_route_visible_changed(true);
                }
            });
        });
    }
    let ui_weak = ui.as_weak();
    let has_entered_chat_once = Rc::new(Cell::new(false));
    let loading_phase_active = Arc::new(AtomicBool::new(false));
    let loading_phase_active_send = loading_phase_active.clone();
    let observed_scroll_bucket = Rc::new(Cell::new(-1));
    let applied_scroll_bucket = Rc::new(Cell::new(-1));
    let auto_load_armed = Rc::new(Cell::new(true));
    let virtual_commit_in_progress = Rc::new(Cell::new(false));
    let virtual_commit_scheduled = Rc::new(Cell::new(false));
    let chat_ui_mount_revision = Rc::new(Cell::new(0u64));
    let layout_revision = Rc::new(Cell::new(0u64));
    let settled_layout_size = Rc::new(Cell::new((0i32, 0i32)));
    let layout_stick_to_bottom = Rc::new(Cell::new(false));

    let ui_weak_mode = ui_weak.clone();
    let session_mode = session.clone();
    let request_state_mode = request_state.clone();
    let drafts_mode = drafts.clone();
    let chat_ui_mount_revision_mode = chat_ui_mount_revision.clone();
    ui.on_chat_switch_mode(move |ai_mode| {
        let Some(ui) = ui_weak_mode.upgrade() else {
            return;
        };
        let previous_mode = chat_mode_from_ui(&ui);
        let mode = if ai_mode { ChatMode::Ai } else { ChatMode::Rag };
        if previous_mode == mode {
            return;
        }
        let next_draft = drafts_mode
            .lock()
            .map(|mut drafts| {
                drafts.set(previous_mode, ui.get_chat_input_text().to_string());
                drafts.get(mode).to_owned()
            })
            .unwrap_or_default();
        if let Ok(mut sessions) = session_mode.lock() {
            // A still-loading session syncs its active mode from the UI once
            // the bootstrap completion handler remounts.
            if let Some(sessions) = sessions.as_mut() {
                sessions.set_active_mode(mode);
            }
        } else {
            log::error!("[Chat::Session] Lock poisoned while switching mode");
            return;
        }
        clear_history_anchor_state();
        clear_virtual_layout_cache();
        clear_render_cache();
        LAST_RENDER_SIGNATURE.with(|last| *last.borrow_mut() = None);
        ui.set_chat_ai_mode(ai_mode);
        ui.set_chat_input_text(next_draft.into());
        ui.set_chat_loading_history(false);
        ui.set_chat_history_load_failed(false);
        let (sending, elapsed_seconds) = request_state_mode
            .lock()
            .map(|state| (state.is_sending(mode), state.elapsed_seconds(mode)))
            .unwrap_or((false, 0));
        ui.set_chat_is_sending(sending);
        ui.set_chat_request_elapsed_seconds(elapsed_seconds.min(i32::MAX as u64) as i32);
        let revision = next_chat_ui_mount_revision(&chat_ui_mount_revision_mode);
        begin_chat_ui_mount(&ui, &session_mode, mode);
        schedule_chat_ui_mount(
            ui.as_weak(),
            session_mode.clone(),
            mode,
            revision,
            chat_ui_mount_revision_mode.clone(),
            true,
        );
    });

    let ui_weak_geometry = ui_weak.clone();
    ui.on_chat_message_geometry(move |message_id, y, height, _position_changed| {
        let revision = GEOMETRY_REVISION.with(|latest| {
            let revision = latest.get().wrapping_add(1);
            latest.set(revision);
            revision
        });
        MESSAGE_GEOMETRIES.with(|geometries| {
            geometries.borrow_mut().insert(
                message_id,
                MessageGeometry {
                    y,
                    height,
                    revision,
                },
            );
        });

        let is_anchor = PENDING_HISTORY_ANCHOR.with(|pending| {
            pending
                .borrow()
                .as_ref()
                .is_some_and(|anchor| anchor.message_id == message_id)
        });
        if is_anchor {
            schedule_anchor_compensation(ui_weak_geometry.clone());
        }
    });

    let ui_weak_user_scroll = ui_weak.clone();
    ui.on_chat_user_scroll_started(move || {
        USER_SCROLL_REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
        let interrupted_anchor = has_pending_anchor();
        clear_pending_anchor();
        if interrupted_anchor {
            // Flickable may report viewport-y before flicked(). Replay that
            // input once after cancellation so a suppressed final wheel event
            // cannot strand the viewport in an unmounted spacer.
            let weak = ui_weak_user_scroll.clone();
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(ui) = weak.upgrade() {
                    ui.invoke_chat_scroll_position_changed(ui.get_chat_scroll_position());
                }
            });
        }
    });

    let ui_weak_scrollbar = ui_weak.clone();
    let session_scrollbar = session.clone();
    let observed_scroll_bucket_scrollbar = observed_scroll_bucket.clone();
    let applied_scroll_bucket_scrollbar = applied_scroll_bucket.clone();
    let virtual_commit_in_progress_scrollbar = virtual_commit_in_progress.clone();
    ui.on_chat_scrollbar_dragging_changed(move |dragging| {
        clear_pending_anchor();
        if dragging {
            return;
        }

        // A direct thumb drag is random access, and pointer-up can arrive before
        // the coalesced scroll callback has consumed its final position. Commit
        // the exact destination once without the wheel path's two-row cap.
        let Some(ui) = ui_weak_scrollbar.upgrade() else {
            return;
        };
        if ui.get_current_route() != Route::Chat
            || ui.get_chat_history_restoring()
            || crate::is_window_interacting()
            || virtual_commit_in_progress_scrollbar.get()
            || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
            || ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(Cell::get)
        {
            return;
        }
        let scroll_bucket = virtual_scroll_bucket((-ui.get_chat_scroll_position()).max(0.0));
        let moving_down = scroll_bucket > applied_scroll_bucket_scrollbar.get();
        observed_scroll_bucket_scrollbar.set(scroll_bucket);
        applied_scroll_bucket_scrollbar.set(scroll_bucket);
        virtual_commit_in_progress_scrollbar.set(true);
        apply_chat_state_to_ui_for_scrollbar_drag(
            &ui,
            &session_scrollbar,
            chat_mode_from_ui(&ui),
            ui.get_chat_is_sending(),
            Some(active_wrap_width_px(&ui)),
            moving_down,
        );
        virtual_commit_in_progress_scrollbar.set(false);
    });

    let ui_weak_layout = ui_weak.clone();
    let session_layout = session.clone();
    let layout_revision_callback = layout_revision.clone();
    let settled_layout_size_callback = settled_layout_size.clone();
    let observed_scroll_bucket_layout = observed_scroll_bucket.clone();
    let applied_scroll_bucket_layout = applied_scroll_bucket.clone();
    let layout_stick_to_bottom_callback = layout_stick_to_bottom.clone();
    ui.on_chat_layout_metrics_changed(move |width, height| {
        let size = (width.round() as i32, height.round() as i32);
        if let Some(ui) = ui_weak_layout.upgrade()
            && ui.get_chat_messages().row_count() > 0
            && ui.get_chat_bottom_distance() <= STICK_TO_BOTTOM_THRESHOLD_PX
        {
            layout_stick_to_bottom_callback.set(true);
        }
        let revision = layout_revision_callback.get().wrapping_add(1);
        layout_revision_callback.set(revision);
        let ui_weak = ui_weak_layout.clone();
        let session = session_layout.clone();
        let latest_revision = layout_revision_callback.clone();
        let settled_size = settled_layout_size_callback.clone();
        let observed_bucket = observed_scroll_bucket_layout.clone();
        let applied_bucket = applied_scroll_bucket_layout.clone();
        let stick_to_bottom_on_settle = layout_stick_to_bottom_callback.clone();
        slint::Timer::single_shot(
            std::time::Duration::from_millis(LAYOUT_SETTLE_DELAY_MS),
            move || {
                if latest_revision.get() != revision || settled_size.get() == size {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if ui.get_current_route() != Route::Chat || crate::is_window_interacting() {
                    return;
                }
                let stick_to_bottom = stick_to_bottom_on_settle.replace(false)
                    || ui.get_chat_bottom_distance() <= STICK_TO_BOTTOM_THRESHOLD_PX;
                if stick_to_bottom {
                    clear_pending_anchor();
                } else {
                    capture_visible_anchor(&ui, AnchorPurpose::SettledLayout, None);
                }
                settled_size.set(size);
                apply_chat_state_to_ui(
                    &ui,
                    &session,
                    chat_mode_from_ui(&ui),
                    ui.get_chat_is_sending(),
                    Some(compute_wrap_width_for_chat_width(width)),
                    false,
                );
                if stick_to_bottom {
                    scroll_chat_to_bottom(&ui);
                }
                let bucket = virtual_scroll_bucket((-ui.get_chat_scroll_position()).max(0.0));
                observed_bucket.set(bucket);
                applied_bucket.set(bucket);
            },
        );
    });

    // Chat's Slint tree and model are route-scoped. The Rust session remains the
    // source of truth and reconstructs the bounded UI model when Chat is entered.
    // At setup time the session is still bootstrapping, so only surface the
    // loading state here; the bootstrap completion handler performs the first
    // mount through the route callback.
    if ui.get_current_route() == Route::Chat {
        ui.set_chat_history_restoring(true);
    } else {
        clear_chat_ui_state(ui);
    }

    // 处理加载更多历史回调
    let ui_weak_load_more = ui_weak.clone();
    let session_load_more = session.clone();
    let runtime_load_more = runtime.clone();
    ui.on_chat_load_more_history(move || {
        if let Some(ui) = ui_weak_load_more.upgrade() {
            if ui.get_chat_loading_history() {
                return;
            }

            clear_pending_anchor();
            ui.set_chat_history_load_failed(false);
            ui.set_chat_loading_history(true);
            let mode = chat_mode_from_ui(&ui);
            log::debug!("[Chat::History] Start loading older messages...");
            let session_for_ui = session_load_more.clone();
            let session_for_task = session_load_more.clone();
            let ui_weak_for_task = ui_weak_load_more.clone();
            runtime_load_more.spawn(async move {
                let io_result = tokio::task::spawn_blocking(move || {
                    let request = {
                        let mut guard = session_for_task
                            .lock()
                            .map_err(|e| format!("session lock poisoned: {e}"))?;
                        let sessions = guard
                            .as_mut()
                            .ok_or_else(|| "chat history is still loading".to_string())?;
                        sessions.prepare_history_load_for(mode)
                    };
                    let Some(request) = request else {
                        return Ok(0);
                    };
                    let page = request
                        .load()
                        .map_err(|e| format!("load history failed: {e}"))?;
                    let mut guard = session_for_task
                        .lock()
                        .map_err(|e| format!("session lock poisoned: {e}"))?;
                    guard
                        .as_mut()
                        .ok_or_else(|| "chat history is still loading".to_string())?
                        .commit_history_load_for(mode, &request, page)
                        .map_err(|e| format!("commit history failed: {e}"))
                })
                .await;

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak_for_task.upgrade() else {
                        return;
                    };

                    match io_result {
                        Ok(Ok(loaded_count)) => {
                            if ui.get_current_route() == Route::Chat
                                && chat_mode_from_ui(&ui) == mode
                            {
                                if loaded_count > 0 {
                                    capture_visible_history_anchor(&ui);
                                }
                                ui.set_chat_history_load_failed(false);
                                ui.set_chat_loading_history(false);
                                let wrap_width_px = active_wrap_width_px(&ui);
                                apply_chat_state_to_ui(
                                    &ui,
                                    &session_for_ui,
                                    mode,
                                    ui.get_chat_is_sending(),
                                    Some(wrap_width_px),
                                    false,
                                );
                            }
                            log::debug!("[Chat::History] History page loaded with visible anchor");
                        }
                        Ok(Err(e)) => {
                            if ui.get_current_route() == Route::Chat
                                && chat_mode_from_ui(&ui) == mode
                            {
                                ui.set_chat_loading_history(false);
                                ui.set_chat_history_load_failed(true);
                            }
                            log::error!("[Chat::Storage] Failed to load older history: {}", e);
                        }
                        Err(e) => {
                            if ui.get_current_route() == Route::Chat
                                && chat_mode_from_ui(&ui) == mode
                            {
                                ui.set_chat_loading_history(false);
                                ui.set_chat_history_load_failed(true);
                            }
                            log::error!("[Chat::Storage] Background load task join failed: {}", e);
                        }
                    }
                });
            });
        }
    });

    // 处理发送消息回调
    let session_send = session.clone();
    let request_state_send = request_state.clone();
    let drafts_send = drafts.clone();
    let runtime_send = runtime.clone();
    ui.on_chat_send_message(move |text: slint::SharedString| {
        let ui_weak_local = ui_weak.clone();

        let Some(ui) = ui_weak_local.upgrade() else {
            log::error!("[Chat::Interactive] Failed to upgrade UI weak reference");
            return;
        };

        let text_str = text.to_string();
        if text_str.trim().is_empty() {
            return;
        }
        if ui.get_chat_is_sending() {
            return;
        }

        let mode = chat_mode_from_ui(&ui);
        let session_ready = session_send
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false);
        if !session_ready {
            log::warn!("[Chat::Storage] Ignoring send while chat history is still loading");
            return;
        }

        let profile = request::capture(&ui, mode);
        let request_started = Instant::now();
        if let Ok(mut drafts) = drafts_send.lock() {
            drafts.set(mode, String::new());
        }
        ui.set_chat_input_text("".into());
        ui.set_chat_is_sending(true);
        ui.set_chat_request_elapsed_seconds(0);
        if let Ok(mut state) = request_state_send.lock() {
            state.start(mode, request_started);
        }

        start_loading_phase_animation(ui.as_weak(), loading_phase_active_send.clone());

        // 用户消息落盘、请求发送与助手消息落盘都在后台执行，保持 UI 线程无文件 IO。
        // 顺序保证：助手消息只有在用户消息落盘完成并收到响应后才会写入。
        let ui_weak_clone = ui_weak.clone();
        let session_response = session_send.clone();
        let request_state_response = request_state_send.clone();
        let drafts_response = drafts_send.clone();
        let loading_phase_active_response = loading_phase_active_send.clone();
        runtime_send.spawn(async move {
            let appended = tokio::task::spawn_blocking({
                let session = session_response.clone();
                let content = text_str.clone();
                move || {
                    let (pending, ai_history) = {
                        let guard = session
                            .lock()
                            .map_err(|e| format!("session lock poisoned: {e}"))?;
                        let sessions = guard
                            .as_ref()
                            .ok_or_else(|| "chat history is still loading".to_string())?;
                        let pending = sessions.prepare_user_message_for(mode, content);
                        let ai_history = if mode == ChatMode::Ai {
                            let mut history = sessions.messages_for(mode).to_vec();
                            history.push(pending.message().clone());
                            ai_completion_messages(&history)
                        } else {
                            Vec::new()
                        };
                        (pending, ai_history)
                    };
                    pending.persist().map_err(|e| e.to_string())?;
                    let mut guard = session
                        .lock()
                        .map_err(|e| format!("session lock poisoned: {e}"))?;
                    guard
                        .as_mut()
                        .ok_or_else(|| "chat history is still loading".to_string())?
                        .commit_message_for(mode, pending)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>(ai_history)
                }
            })
            .await
            .map_err(|e| format!("persist task join failed: {e}"))
            .and_then(|result| result);

            let ai_history = match appended {
                Ok(history) => history,
                Err(e) => {
                    log::error!("[Chat::Storage] Failed to persist user message: {}", e);
                    let ui_weak_failed = ui_weak_clone.clone();
                    let request_state_failed = request_state_response.clone();
                    let drafts_failed = drafts_response.clone();
                    let loading_phase_failed = loading_phase_active_response.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Ok(mut state) = request_state_failed.lock() {
                            state.finish(mode);
                        }
                        let Some(ui) = ui_weak_failed.upgrade() else {
                            return;
                        };
                        if let Ok(mut drafts) = drafts_failed.lock() {
                            drafts.set(mode, text_str.clone());
                        }
                        if chat_mode_from_ui(&ui) == mode {
                            loading_phase_failed.store(false, Ordering::Relaxed);
                            ui.set_chat_input_text(text_str.clone().into());
                            ui.set_chat_is_sending(false);
                            ui.set_chat_loading_phase(0);
                            ui.set_chat_request_elapsed_seconds(0);
                        }
                        request::finish(&ui, mode);
                        show_chat_storage_error(&ui);
                    });
                    return;
                }
            };

            // 用户消息已持久化，先在 UI 上呈现。
            {
                let ui_weak_render = ui_weak_clone.clone();
                let session_render = session_response.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak_render.upgrade() else {
                        return;
                    };
                    if ui.get_current_route() == Route::Chat && chat_mode_from_ui(&ui) == mode {
                        apply_chat_state_to_ui(
                            &ui,
                            &session_render,
                            mode,
                            true,
                            Some(active_wrap_width_px(&ui)),
                            false,
                        );
                        scroll_chat_to_bottom(&ui);
                    }
                });
            }

            let identity = profile
                .as_ref()
                .map(crate::ai::identity::AgentIdentity::from_profile);
            let result = request::send_chat_message(
                mode,
                &text_str,
                ai_history,
                profile.map(|profile| profile.connection),
            )
            .await;
            let elapsed_ms = request_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;

            let persisted = tokio::task::spawn_blocking({
                let session = session_response.clone();
                move || {
                    let pending = {
                        let guard = session
                            .lock()
                            .map_err(|e| format!("session lock poisoned: {e}"))?;
                        let sessions = guard
                            .as_ref()
                            .ok_or_else(|| "chat session unavailable".to_string())?;
                        let mut pending = match result {
                            Ok(response) => sessions.prepare_assistant_message_for(
                                mode,
                                response,
                                StoredMessageStatus::Normal,
                                elapsed_ms,
                            ),
                            Err(err) => {
                                let friendly = handle_api_error(&err);
                                sessions.prepare_assistant_message_for(
                                    mode,
                                    friendly,
                                    StoredMessageStatus::Error,
                                    elapsed_ms,
                                )
                            }
                        };
                        pending.set_agent_identity(identity);
                        pending
                    };
                    pending.persist().map_err(|e| e.to_string())?;
                    let mut guard = session
                        .lock()
                        .map_err(|e| format!("session lock poisoned: {e}"))?;
                    guard
                        .as_mut()
                        .ok_or_else(|| "chat session unavailable".to_string())?
                        .commit_message_for(mode, pending)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
            })
            .await
            .map_err(|e| format!("persist task join failed: {e}"))
            .and_then(|result| result);
            if let Err(e) = persisted {
                log::error!("[Chat::Storage] Failed to persist assistant message: {}", e);
            }

            let _ = slint::invoke_from_event_loop(move || {
                if let Ok(mut state) = request_state_response.lock() {
                    state.finish(mode);
                }
                let Some(ui) = ui_weak_clone.upgrade() else {
                    return;
                };
                request::finish(&ui, mode);
                if chat_mode_from_ui(&ui) == mode {
                    loading_phase_active_response.store(false, Ordering::Relaxed);
                    ui.set_chat_is_sending(false);
                    ui.set_chat_loading_phase(0);
                    ui.set_chat_request_elapsed_seconds(0);
                }
                if ui.get_current_route() == Route::Chat && chat_mode_from_ui(&ui) == mode {
                    apply_chat_state_to_ui(
                        &ui,
                        &session_response,
                        mode,
                        false,
                        Some(active_wrap_width_px(&ui)),
                        false,
                    );
                    scroll_chat_to_bottom(&ui);
                }
            });
        });
    });

    // Scroll virtualization is driven by actual viewport changes. This keeps a
    // final delayed poll from replacing the model after wheel motion stops.
    let initial_scroll_bucket = {
        let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
        virtual_scroll_bucket(scroll_offset_px)
    };
    observed_scroll_bucket.set(initial_scroll_bucket);
    applied_scroll_bucket.set(initial_scroll_bucket);

    let ui_weak_scroll = ui.as_weak();
    let last_scroll_offset = Rc::new(Cell::new((-ui.get_chat_scroll_position()).max(0.0)));
    let latest_scroll_down = Rc::new(Cell::new(false));
    let session_scroll = session.clone();
    let observed_scroll_bucket_scroll = observed_scroll_bucket.clone();
    let applied_scroll_bucket_scroll = applied_scroll_bucket.clone();
    let auto_load_armed_scroll = auto_load_armed.clone();
    let virtual_commit_in_progress_scroll = virtual_commit_in_progress.clone();
    let virtual_commit_scheduled_scroll = virtual_commit_scheduled.clone();
    let layout_stick_to_bottom_scroll = layout_stick_to_bottom.clone();
    ui.on_chat_scroll_position_changed(move |position| {
        let scroll_offset_px = (-position).max(0.0);
        let previous_offset = last_scroll_offset.replace(scroll_offset_px);
        if (scroll_offset_px - previous_offset).abs() > 0.01 {
            latest_scroll_down.set(scroll_offset_px > previous_offset);
        }
        let Some(ui) = ui_weak_scroll.upgrade() else {
            return;
        };
        if ui.get_current_route() != Route::Chat
            || ui.get_chat_history_restoring()
            || crate::is_window_interacting()
            || virtual_commit_in_progress_scroll.get()
            || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
            || ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(Cell::get)
            || has_pending_anchor()
        {
            return;
        }

        if ui.get_chat_bottom_distance() > STICK_TO_BOTTOM_THRESHOLD_PX {
            layout_stick_to_bottom_scroll.set(false);
        }
        let (next_auto_load_armed, should_auto_load) =
            update_auto_load_armed(auto_load_armed_scroll.get(), scroll_offset_px);
        auto_load_armed_scroll.set(next_auto_load_armed);
        if should_auto_load && ui.get_chat_can_load_more_history() && !ui.get_chat_loading_history()
        {
            ui.invoke_chat_load_more_history();
        }

        let scroll_bucket = virtual_scroll_bucket(scroll_offset_px);
        observed_scroll_bucket_scroll.set(scroll_bucket);
        let applied = applied_scroll_bucket_scroll.get();
        if applied < 0 {
            applied_scroll_bucket_scroll.set(scroll_bucket);
            return;
        }
        let moving_down = latest_scroll_down.get();
        let viewport_height = ui.get_chat_message_viewport_height().max(1.0);
        let (_, near_virtual_edge) = MESSAGE_GEOMETRIES.with(|geometries| {
            should_shift_near_virtual_edge(
                &ui,
                &geometries.borrow(),
                scroll_offset_px,
                viewport_height,
                moving_down,
                virtual_window_has_more(moving_down),
            )
        });
        if !near_virtual_edge
            && !should_shift_virtual_window(applied, scroll_bucket, scroll_update_bucket_step())
        {
            return;
        }
        if virtual_commit_scheduled_scroll.replace(true) {
            return;
        }

        let ui_weak = ui_weak_scroll.clone();
        let session = session_scroll.clone();
        let applied_bucket = applied_scroll_bucket_scroll.clone();
        let commit_scheduled = virtual_commit_scheduled_scroll.clone();
        let commit_in_progress = virtual_commit_in_progress_scroll.clone();
        let latest_scroll_down = latest_scroll_down.clone();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            commit_scheduled.set(false);
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_current_route() != Route::Chat
                || ui.get_chat_history_restoring()
                || crate::is_window_interacting()
                || commit_in_progress.get()
                || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
                || ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(Cell::get)
                || has_pending_anchor()
            {
                return;
            }

            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let scroll_bucket = virtual_scroll_bucket(scroll_offset_px);
            let applied = applied_bucket.get();
            if applied < 0 {
                applied_bucket.set(scroll_bucket);
                return;
            }
            let moving_down = latest_scroll_down.get();
            let viewport_height = ui.get_chat_message_viewport_height().max(1.0);
            let (near_mounted_edge, near_virtual_edge) = MESSAGE_GEOMETRIES.with(|geometries| {
                should_shift_near_virtual_edge(
                    &ui,
                    &geometries.borrow(),
                    scroll_offset_px,
                    viewport_height,
                    moving_down,
                    virtual_window_has_more(moving_down),
                )
            });
            if !near_virtual_edge
                && !should_shift_virtual_window(applied, scroll_bucket, scroll_update_bucket_step())
            {
                return;
            }

            if crate::chat::performance::variant().is_optimized()
                || ui.get_chat_scrollbar_dragging()
            {
                clear_pending_anchor();
            } else {
                capture_visible_anchor(
                    &ui,
                    AnchorPurpose::VirtualWindow,
                    Some(MOUNTED_EDGE_GUARD_PX),
                );
            }
            applied_bucket.set(scroll_bucket);
            commit_in_progress.set(true);
            if ui.get_chat_scrollbar_dragging() {
                apply_chat_state_to_ui_for_scrollbar_drag(
                    &ui,
                    &session,
                    chat_mode_from_ui(&ui),
                    ui.get_chat_is_sending(),
                    Some(active_wrap_width_px(&ui)),
                    moving_down,
                );
            } else {
                apply_chat_state_to_ui(
                    &ui,
                    &session,
                    chat_mode_from_ui(&ui),
                    ui.get_chat_is_sending(),
                    Some(active_wrap_width_px(&ui)),
                    // Estimated heights may be taller than the real mounted
                    // rows. Once the real edge guard fires, converge directly
                    // instead of leaving a two-row partial window in view.
                    !near_mounted_edge,
                );
            }
            // Model assignment can synchronously emit geometry callbacks, so
            // guard only that stack. The zero-delay scheduler coalesces input
            // without leaving an idle poll after wheel motion stops.
            commit_in_progress.set(false);
        });
    });

    let ui_weak_route = ui.as_weak();
    let session_route = session.clone();
    let observed_scroll_bucket_route = observed_scroll_bucket.clone();
    let applied_scroll_bucket_route = applied_scroll_bucket.clone();
    let auto_load_armed_route = auto_load_armed.clone();
    let loading_phase_active_route = loading_phase_active.clone();
    let has_entered_chat_once_route = has_entered_chat_once.clone();
    let request_state_route = request_state.clone();
    let chat_ui_mount_revision_route = chat_ui_mount_revision.clone();

    ui.on_chat_route_visible_changed(move |visible| {
        if visible {
            let first_entry = !has_entered_chat_once_route.replace(true);
            if let Some(ui) = ui_weak_route.upgrade() {
                let mode = chat_mode_from_ui(&ui);
                if let Ok(mut sessions) = session_route.lock()
                    && let Some(sessions) = sessions.as_mut()
                {
                    sessions.set_active_mode(mode);
                }
                let (sending, elapsed_seconds) = request_state_route
                    .lock()
                    .map(|state| (state.is_sending(mode), state.elapsed_seconds(mode)))
                    .unwrap_or((false, 0));
                ui.set_chat_is_sending(sending);
                ui.set_chat_request_elapsed_seconds(elapsed_seconds.min(i32::MAX as u64) as i32);
                request::sync_agent_identity(&ui);
                let revision = next_chat_ui_mount_revision(&chat_ui_mount_revision_route);
                begin_chat_ui_mount(&ui, &session_route, mode);
                schedule_chat_ui_mount(
                    ui.as_weak(),
                    session_route.clone(),
                    mode,
                    revision,
                    chat_ui_mount_revision_route.clone(),
                    first_entry,
                );
            }
            if let Some(ui) = ui_weak_route.upgrade() {
                let bucket = virtual_scroll_bucket((-ui.get_chat_scroll_position()).max(0.0));
                observed_scroll_bucket_route.set(bucket);
                applied_scroll_bucket_route.set(bucket);
                auto_load_armed_route.set(true);
            }
        } else {
            next_chat_ui_mount_revision(&chat_ui_mount_revision_route);
            clear_history_anchor_state();
            clear_virtual_layout_cache();
            stop_loading_phase_animation(ui_weak_route.clone(), loading_phase_active_route.clone());
            LAST_RENDER_SIGNATURE.with(|last| {
                *last.borrow_mut() = None;
            });
            clear_render_cache();
            if let Some(ui) = ui_weak_route.upgrade() {
                clear_chat_ui_state(&ui);
            }
        }
    });

    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}

#[cfg(test)]
#[path = "pagination_tests.rs"]
mod pagination_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_scrollbar_origin_is_local_to_the_message_viewport() {
        let chat_view = include_str!("../../ui/views/chat.slint");
        let main_ui = include_str!("../../ui/main.slint");

        assert!(!chat_view.contains("in property <length> scrollbar-top-inset"));
        assert!(!chat_view.contains("scrollbar-top-inset: root.scrollbar-top-inset"));
        assert!(!main_ui.contains("scrollbar-top-inset: root.native-titlebar-inset"));
    }

    #[test]
    fn drafts_are_isolated_by_chat_mode() {
        let mut drafts = ChatDrafts::default();
        drafts.set(ChatMode::Ai, "AI draft".into());
        drafts.set(ChatMode::Rag, "RAG draft".into());

        assert_eq!(drafts.get(ChatMode::Ai), "AI draft");
        assert_eq!(drafts.get(ChatMode::Rag), "RAG draft");
    }

    #[test]
    fn virtual_scroll_bucket_changes_only_with_scroll_position() {
        assert_eq!(virtual_scroll_bucket(0.0), 0);
        assert_eq!(virtual_scroll_bucket(63.9), 0);
        assert_eq!(virtual_scroll_bucket(64.0), 1);
        assert_eq!(virtual_scroll_bucket(1_024.0), 16);
    }

    #[test]
    fn rapid_mode_switch_rejects_stale_delayed_mounts() {
        let revision = Cell::new(0);
        let initial_ai = next_chat_ui_mount_revision(&revision);
        let rag = next_chat_ui_mount_revision(&revision);
        let latest_ai = next_chat_ui_mount_revision(&revision);

        assert!(!chat_ui_mount_is_current(
            ChatMode::Ai,
            initial_ai,
            ChatMode::Ai,
            revision.get(),
        ));
        assert!(!chat_ui_mount_is_current(
            ChatMode::Rag,
            rag,
            ChatMode::Ai,
            revision.get(),
        ));
        assert!(chat_ui_mount_is_current(
            ChatMode::Ai,
            latest_ai,
            ChatMode::Ai,
            revision.get(),
        ));
    }

    #[test]
    fn contiguous_overlap_preserves_the_stable_middle_window() {
        assert_eq!(
            longest_contiguous_overlap(&[10, 11, 12, 13], &[12, 13, 14, 15]),
            Some((2, 0, 2))
        );
        assert_eq!(
            longest_contiguous_overlap(&[12, 13, 14, 15], &[10, 11, 12, 13]),
            Some((0, 2, 2))
        );
        assert_eq!(longest_contiguous_overlap(&[1, 2], &[3, 4]), None);
        assert_eq!(longest_contiguous_overlap(&[], &[1]), None);
    }

    #[test]
    fn identical_message_ids_from_different_modes_never_reuse_rows() {
        let ai = [
            ChatMessageRowKey {
                mode: ChatMode::Ai,
                id: 1,
            },
            ChatMessageRowKey {
                mode: ChatMode::Ai,
                id: 2,
            },
        ];
        let rag = [
            ChatMessageRowKey {
                mode: ChatMode::Rag,
                id: 1,
            },
            ChatMessageRowKey {
                mode: ChatMode::Rag,
                id: 2,
            },
        ];

        assert_eq!(longest_contiguous_overlap(&ai, &rag), None);
        assert_eq!(longest_contiguous_overlap(&ai, &ai), Some((0, 0, 2)));
    }

    #[test]
    fn virtual_window_shift_requires_the_full_pixel_threshold() {
        assert!(!should_shift_virtual_window(0, 15, 16));
        assert!(should_shift_virtual_window(0, 16, 16));
        assert!(!should_shift_virtual_window(30, 15, 16));
        assert!(should_shift_virtual_window(30, 14, 16));
        assert!(!should_shift_virtual_window(0, 63, 64));
        assert!(should_shift_virtual_window(0, 64, 64));
        assert!(!should_shift_virtual_window(0, 127, 128));
        assert!(should_shift_virtual_window(0, 128, 128));
    }

    fn render_signature(start: usize, end: usize) -> VirtualRenderSignature {
        VirtualRenderSignature {
            total_len: 100,
            start,
            end,
            include_loading_placeholder: false,
        }
    }

    #[test]
    fn incremental_shift_caps_new_rows_without_shrinking_the_overlap_to_one_row() {
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 17, 30, 2),
            (12, 25)
        );
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 3, 16, 2),
            (8, 21)
        );
    }

    #[test]
    fn incremental_shift_keeps_deletions_and_non_overlapping_jumps_exact() {
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 12, 20, 2),
            (12, 20)
        );
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 30, 38, 2),
            (30, 38)
        );
    }

    #[test]
    fn estimated_window_guard_does_not_depend_on_mounted_geometry_callbacks() {
        assert!(should_shift_near_estimated_edge(
            4_000.0, 10_000.0, 20_000.0, 9_280.0, 720.0, true, true,
        ));
        assert!(!should_shift_near_estimated_edge(
            4_000.0, 10_000.0, 20_000.0, 9_278.0, 720.0, true, true,
        ));
    }

    #[test]
    fn incremental_window_must_cover_a_fast_scroll_destination() {
        assert!(estimated_window_covers_viewport(
            4_000.0, 10_000.0, 20_000.0, 6_000.0, 720.0,
        ));
        assert!(!estimated_window_covers_viewport(
            4_000.0, 10_000.0, 20_000.0, 9_600.0, 720.0,
        ));
    }

    #[test]
    fn visible_anchor_prefers_the_first_fully_visible_message() {
        let geometries = HashMap::from([
            (
                10,
                MessageGeometry {
                    y: 80.0,
                    height: 90.0,
                    revision: 1,
                },
            ),
            (
                11,
                MessageGeometry {
                    y: 178.0,
                    height: 60.0,
                    revision: 2,
                },
            ),
            (
                12,
                MessageGeometry {
                    y: 246.0,
                    height: 70.0,
                    revision: 3,
                },
            ),
        ]);

        assert_eq!(
            select_visible_anchor(&geometries, 150.0, 100.0),
            Some(VisibleAnchor {
                message_id: 11,
                content_y: 178.0,
                scroll_offset: 150.0,
                geometry_revision: 2,
            })
        );
    }

    #[test]
    fn visible_anchor_falls_back_to_the_top_intersecting_long_message() {
        let geometries = HashMap::from([
            (
                10,
                MessageGeometry {
                    y: 100.0,
                    height: 1_000.0,
                    revision: 4,
                },
            ),
            (
                11,
                MessageGeometry {
                    y: 1_108.0,
                    height: 60.0,
                    revision: 5,
                },
            ),
        ]);

        assert_eq!(
            select_visible_anchor(&geometries, 150.0, 600.0),
            Some(VisibleAnchor {
                message_id: 10,
                content_y: 100.0,
                scroll_offset: 150.0,
                geometry_revision: 4,
            })
        );
    }

    fn test_anchor_settle(max_correction_px: Option<f32>) -> AnchorSettle {
        let anchor = VisibleAnchor {
            message_id: 11,
            content_y: 178.0,
            scroll_offset: 150.0,
            geometry_revision: 7,
        };
        AnchorSettle::new(
            9,
            ChatMode::Ai,
            AnchorPurpose::HistoryPrepend,
            anchor,
            3,
            max_correction_px,
        )
    }

    #[test]
    fn prepend_anchor_waits_for_new_geometry_and_settles_across_frames() {
        let mut settle = test_anchor_settle(None);
        let old_geometry = MessageGeometry {
            y: 178.0,
            height: 60.0,
            revision: 7,
        };
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 150.0, Some(old_geometry)),
            AnchorStep::Wait
        );

        let first_layout = MessageGeometry {
            y: 438.0,
            height: 60.0,
            revision: 8,
        };
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 150.0, Some(first_layout)),
            AnchorStep::Write(410.0)
        );

        let final_layout = MessageGeometry {
            y: 468.0,
            height: 60.0,
            revision: 9,
        };
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 410.0, Some(final_layout)),
            AnchorStep::Write(440.0)
        );
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 440.0, Some(final_layout)),
            AnchorStep::Wait
        );
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 440.0, Some(final_layout)),
            AnchorStep::Complete
        );
        assert_eq!(final_layout.y - 440.0, settle.viewport_y);
    }

    #[test]
    fn user_input_or_mode_switch_cancels_a_pending_anchor() {
        let mut input_changed = test_anchor_settle(None);
        assert_eq!(
            input_changed.step(ChatMode::Ai, 4, 150.0, None),
            AnchorStep::Cancel
        );

        let mut mode_changed = test_anchor_settle(None);
        assert_eq!(
            mode_changed.step(ChatMode::Rag, 3, 150.0, None),
            AnchorStep::Cancel
        );

        let mut unexpected_scroll = test_anchor_settle(None);
        assert_eq!(
            unexpected_scroll.step(ChatMode::Ai, 3, 180.0, None),
            AnchorStep::Cancel
        );
    }

    #[test]
    fn implausible_virtual_anchor_correction_is_rejected() {
        let mut settle = test_anchor_settle(Some(2_400.0));
        let implausible = MessageGeometry {
            y: 20_000.0,
            height: 60.0,
            revision: 8,
        };
        assert_eq!(
            settle.step(ChatMode::Ai, 3, 150.0, Some(implausible)),
            AnchorStep::Cancel
        );
    }

    #[test]
    fn history_prepend_mounts_predecessors_and_preserves_the_anchor() {
        let previous = VirtualRenderSignature {
            total_len: 20,
            start: 0,
            end: 20,
            include_loading_placeholder: false,
        };

        assert_eq!(shifted_prepend_window(previous, 30, 10), Some((0, 30)));
        assert_eq!(shifted_prepend_window(previous, 30, 9), None);
        assert_eq!(
            shifted_prepend_window(previous, 20, 10),
            None,
            "a non-prepend render must use the ordinary virtual window"
        );
    }

    #[test]
    fn mounted_edge_guard_shifts_before_the_viewport_enters_a_spacer() {
        let geometries = HashMap::from([
            (
                20,
                MessageGeometry {
                    y: 10_000.0,
                    height: 180.0,
                    revision: 1,
                },
            ),
            (
                21,
                MessageGeometry {
                    y: 10_188.0,
                    height: 220.0,
                    revision: 2,
                },
            ),
        ]);

        assert!(!should_shift_near_mounted_edge(
            &geometries,
            7_000.0,
            720.0,
            true,
            true,
        ));
        assert!(should_shift_near_mounted_edge(
            &geometries,
            8_000.0,
            720.0,
            true,
            true,
        ));
        assert!(!should_shift_near_mounted_edge(
            &geometries,
            8_000.0,
            720.0,
            true,
            false,
        ));
    }

    #[test]
    fn blank_exposure_is_zero_when_real_rows_cover_the_viewport() {
        LAST_RENDER_SIGNATURE.with(|last| {
            *last.borrow_mut() = Some(VirtualRenderSignature {
                total_len: 10,
                start: 2,
                end: 6,
                include_loading_placeholder: false,
            });
        });
        MESSAGE_GEOMETRIES.with(|geometries| {
            *geometries.borrow_mut() = HashMap::from([(
                3,
                MessageGeometry {
                    y: 1_000.0,
                    height: 2_000.0,
                    revision: 1,
                },
            )]);
        });
        assert_eq!(virtual_blank_exposure_px(1_200.0, 720.0), 0.0);
        assert_eq!(virtual_blank_exposure_px(2_800.0, 720.0), 520.0);
        clear_history_anchor_state();
        LAST_RENDER_SIGNATURE.with(|last| *last.borrow_mut() = None);
    }

    #[test]
    fn automatic_history_load_requires_leaving_and_reentering_the_top_zone() {
        assert_eq!(update_auto_load_armed(true, 150.0), (false, true));
        assert_eq!(update_auto_load_armed(false, 0.0), (false, false));
        assert_eq!(update_auto_load_armed(false, 321.0), (true, false));
        assert_eq!(update_auto_load_armed(true, 159.0), (false, true));
    }
}
