//! Host-owned focus for the independently generated Inbox tree.

use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::ParentItemTraversalMode;
use i_slint_core::window::WindowInner;
use slint::ComponentHandle;
use slint::platform::Key;
use usher_messages::GitHubMessagesPage;

use crate::MainWindow;

pub(super) fn bind(page: &GitHubMessagesPage, window: &MainWindow) {
    let weak_window = window.as_weak();
    let weak_page = page.as_weak();
    page.on_host_key_pressed(move |text, control, alt, shift, meta| {
        let (Some(window), Some(page)) = (weak_window.upgrade(), weak_page.upgrade()) else {
            return false;
        };
        if !page.get_active() || window.get_modal_overlay_visible() {
            return true;
        }
        // Static Rust factory roots have no parent link to app-shortcut-scope.
        if window.invoke_app_shortcut_key_pressed(text.clone(), control, alt, shift, meta) {
            return true;
        }
        let backtab = text == slint::SharedString::from(Key::Backtab);
        if (text == slint::SharedString::from(Key::Tab) || backtab) && !control && !alt && !meta {
            return crate::focus_navigation::move_tab(&window, shift || backtab);
        }
        false
    });
}

pub(super) fn release(page: &GitHubMessagesPage, window: &MainWindow) {
    let private = WindowInner::from_pub(page.window());
    let pending = private.focus_item.borrow().upgrade();
    if let Some(pending) = pending {
        private.set_focus_item(&pending, false, FocusReason::Programmatic);
    }
    let host = WindowInner::from_pub(window.window());
    let Some(focused) = host.focus_item.borrow().upgrade() else {
        return;
    };
    let page_tree = WindowInner::from_pub(page.window()).component();
    let mut ancestor = Some(focused.clone());
    while let Some(item) = ancestor {
        if i_slint_core::item_tree::ItemTreeRc::ptr_eq(item.item_tree(), &page_tree) {
            host.set_focus_item(&focused, false, FocusReason::Programmatic);
            return;
        }
        ancestor = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
}

pub(super) fn transfer_pointer(page: &GitHubMessagesPage, window: &MainWindow) {
    // Generated .focus() uses the page's private adapter. Move that actual item
    // to the host before changing rows; keyboard activation already belongs to
    // the host and never creates a second private focus owner.
    let private = WindowInner::from_pub(page.window());
    let Some(focused) = private.focus_item.borrow().upgrade() else {
        return;
    };
    let host = WindowInner::from_pub(window.window());
    host.set_focus_item(&focused, false, FocusReason::Programmatic);
    private.set_focus_item(&focused, false, FocusReason::Programmatic);
    host.set_focus_item(&focused, true, FocusReason::Programmatic);
}

pub(super) fn owns_page_focus(page: &GitHubMessagesPage, window: &MainWindow) -> bool {
    let host = WindowInner::from_pub(window.window());
    let Some(focused) = host.focus_item.borrow().upgrade() else {
        return false;
    };
    let page_tree = WindowInner::from_pub(page.window()).component();
    let mut ancestor = Some(focused);
    while let Some(item) = ancestor {
        if i_slint_core::item_tree::ItemTreeRc::ptr_eq(item.item_tree(), &page_tree) {
            return true;
        }
        ancestor = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    false
}
