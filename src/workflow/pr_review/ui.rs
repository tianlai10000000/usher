//! The single PR Review UI owner; no copied page models or root callback proxies.
use slint::ComponentHandle;

#[path = "ui/context.rs"]
mod context;
#[cfg(not(feature = "live-preview"))]
#[path = "ui/input.rs"]
mod input;

#[cfg(not(feature = "live-preview"))]
pub type PrReviewUiRoot = usher_ui_pr_review::PrReviewFactory;
#[cfg(feature = "live-preview")]
pub type PrReviewUiRoot = crate::MainWindow;

#[cfg(not(feature = "live-preview"))]
thread_local! {
    static VIEWS: std::cell::RefCell<Vec<(slint::Weak<crate::MainWindow>, slint::Weak<PrReviewUiRoot>)>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(feature = "live-preview")]
thread_local! {
    static INSTALLED: std::cell::RefCell<Vec<slint::Weak<crate::MainWindow>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Access the actual PR Review root; this never copies presentation models.
pub trait PrReviewUiExt {
    fn pr_review(&self) -> PrReviewUiRoot;
}

impl PrReviewUiExt for crate::MainWindow {
    fn pr_review(&self) -> PrReviewUiRoot {
        view(self)
    }
}

pub fn install(window: &crate::MainWindow) {
    let _ = view(window);
}

/// Native input can precede Slint's queued `changed` publication. Refresh only
/// the feature's critical modal summary, without materializing or copying models.
pub(crate) fn synchronize_modal_input(window: &crate::MainWindow) {
    if window.get_current_route() == crate::Route::PrReview {
        context::synchronize_modal(window, &view(window));
    }
}

/// Resolve the feature's actual UI root. All callers run on the event loop.
/// The static factory closure retains one root; the registry keeps only weak
/// handles, so dropping MainWindow releases the models and callback graph.
#[cfg(not(feature = "live-preview"))]
pub fn view(window: &crate::MainWindow) -> PrReviewUiRoot {
    if let Some(page) = VIEWS.with(|views| {
        views.borrow().iter().find_map(|(host, page)| {
            let host = host.upgrade()?;
            std::ptr::eq(host.window(), window.window())
                .then(|| page.upgrade())
                .flatten()
        })
    }) {
        return page;
    }
    let host = i_slint_core::window::WindowInner::from_pub(window.window());
    let shell = host.component();
    let result = PrReviewUiRoot::new_with_existing_window(window.window());
    host.set_component(&shell);
    let page = result.expect("failed to create PR Review UI");
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|(host, _)| host.upgrade().is_some());
        views.push((window.as_weak(), page.as_weak()));
    });
    let retained = page.clone_strong();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .set_factory(slint::ComponentFactory::new(move |_| {
            Some(retained.clone_strong())
        }));
    context::bind(window, &page);
    input::bind(window, &page);
    context::sync(window, &page);
    page
}

#[cfg(feature = "live-preview")]
pub fn view(window: &crate::MainWindow) -> PrReviewUiRoot {
    let first = INSTALLED.with(|items| {
        let mut items = items.borrow_mut();
        items.retain(|item| item.upgrade().is_some());
        if items.iter().any(|item| {
            item.upgrade()
                .is_some_and(|host| std::ptr::eq(host.window(), window.window()))
        }) {
            false
        } else {
            items.push(window.as_weak());
            true
        }
    });
    if first {
        context::bind(window, window);
        context::sync(window, window);
    }
    window.clone_strong()
}
