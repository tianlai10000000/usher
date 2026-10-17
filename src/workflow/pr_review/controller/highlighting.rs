//! One active highlight and one replaceable pending input per controller.
use super::highlight_cache::HighlightResourceCache;
use super::highlight_session::HighlightLease;
use super::*;
use crate::app_runtime::latest_compute::LatestCompute;
use crate::workflow::syntax_highlight::highlight_diff_lines_cancellable;
use std::sync::OnceLock;

struct Request {
    state: ReviewStateFacade,
    generation: u64,
    index: usize,
    file: SharedDiffFile,
    resources: HighlightLease<HighlightResources>,
    cached_resources: HighlightLease<HighlightResources>,
    full_file: bool,
    reset_scroll: bool,
}

struct Completion {
    request: Request,
    highlighted: Result<Vec<HighlightedLine>, ()>,
}

#[derive(Clone, Default)]
pub(super) struct HighlightWorker {
    engine: Arc<OnceLock<LatestCompute<Request, Completion>>>,
    resources: Arc<HighlightResourceCache<HighlightResources>>,
}

impl HighlightWorker {
    fn engine(&self) -> &LatestCompute<Request, Completion> {
        self.engine.get_or_init(LatestCompute::new)
    }

    pub(super) fn resource_cache(&self) -> &Arc<HighlightResourceCache<HighlightResources>> {
        &self.resources
    }

    pub(super) fn cancel(&self) {
        if let Some(worker) = self.engine.get() {
            worker.cancel();
        }
    }
}

fn compute(request: Request, mut cancelled: impl FnMut() -> bool) -> Option<Completion> {
    // Capture leases on the UI loop, then reject obsolete work BEFORE resource
    // initialization. An old queued request cannot fill a replacement slot.
    if cancelled()
        || !request
            .state
            .is_current(ReviewRequestKind::Highlight, request.generation)
    {
        return None;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let resources = request.resources.get_or_init(|| {
            request
                .cached_resources
                .get_or_init(|| Arc::new(HighlightResources::load()))
                .clone()
        });
        if cancelled() {
            return None;
        }
        highlight_diff_lines_cancellable(
            resources,
            request.file.path.as_str(),
            &request.file.lines,
            &mut cancelled,
        )
    }));
    if cancelled() {
        return None;
    }
    let highlighted = match result {
        Ok(Some(lines)) => Ok(lines),
        Ok(None) => return None,
        Err(_) => Err(()),
    };
    Some(Completion {
        request,
        highlighted,
    })
}

impl WorkflowPrReviewController {
    pub(super) fn begin_highlight(&self) -> u64 {
        // Cache hits also invalidate in-flight work even though they submit no
        // replacement calculation. Deferred files use invalidate_highlight().
        self.highlighting.cancel();
        self.state.begin(ReviewRequestKind::Highlight)
    }

    pub(super) fn invalidate_highlight(&self) {
        self.highlighting.cancel();
        self.state.invalidate(ReviewRequestKind::Highlight);
    }

    pub(super) fn schedule_highlight(
        &self,
        ui: &MainWindow,
        generation: u64,
        index: usize,
        file: SharedDiffFile,
        full_file: bool,
        reset_scroll: bool,
    ) {
        let request = Request {
            state: self.state.clone(),
            generation,
            index,
            file,
            // This must not move into compute(): reset may replace the slot
            // between scheduling and execution on the blocking pool.
            resources: self.highlight_resources.lease(),
            cached_resources: self.highlighting.resource_cache().lease(),
            full_file,
            reset_scroll,
        };
        let worker = self.highlighting.engine();
        let completion_worker = worker.clone();
        let controller = self.clone();
        let weak = ui.as_weak();
        worker.submit(
            request,
            |request, token| compute(request, || token.cancelled()),
            move || {
                let weak = weak.clone();
                let worker = completion_worker.clone();
                let controller = controller.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    worker.acknowledge();
                    let Some(completed) = worker.take_output() else {
                        return;
                    };
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    controller.apply_highlight_completion(&ui, completed);
                });
            },
        );
    }

    fn apply_highlight_completion(&self, ui: &MainWindow, completion: Completion) {
        let Request {
            generation,
            index,
            file,
            full_file,
            reset_scroll,
            ..
        } = completion.request;
        // Keep selected index/path/mode and domain-generation validation in the
        // established authority; a mailbox token is not a UI identity check.
        if !highlight_completion::finish(&self.state, ui, generation, index, &file.path, full_file)
        {
            return;
        }
        let highlighted = match completion.highlighted {
            Ok(lines) => Arc::new(lines),
            Err(()) => {
                if full_file {
                    log::warn!("[Workflow][PR Review] Full-file highlighting panicked");
                } else {
                    ui.pr_review()
                        .global::<crate::PrReviewBridge>()
                        .set_workflow_pr_review_error(
                            crate::i18n::tr("pr_review.syntax_highlight_failed")
                                .into_owned()
                                .into(),
                        );
                }
                return;
            }
        };
        if !full_file && let Ok(mut cached) = self.highlighted_files.lock() {
            cached.insert(file.path.clone(), highlighted.clone());
        }
        apply_highlighted_file(ui, file, highlighted, reset_scroll);
        self.log_memory_diagnostics(if full_file {
            "full_file.highlighted_and_applied"
        } else {
            "file_highlight.generated_and_applied"
        });
        if !full_file {
            self.continue_location_reveal(ui);
        }
    }
}

#[cfg(test)]
#[path = "highlighting_tests.rs"]
mod tests;
