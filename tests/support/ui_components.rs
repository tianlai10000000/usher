//! Real playground installation and scrolling for static and integrated roots.
use i_slint_backend_testing::ElementHandle;
use i_slint_core::item_tree::{ItemRc, ItemVisitorResult, TraversalOrder, visit_items};
use i_slint_core::window::WindowInner;
use slint::{ComponentHandle, LogicalPosition, platform::WindowEvent};
use std::time::Duration;
use usher_lib::MainWindow;

pub fn install(_window: &MainWindow) {
    #[cfg(not(feature = "live-preview"))]
    usher_lib::install_ui_components_fixture(_window);
}

pub fn document(window: &MainWindow) -> ElementHandle {
    let id = if cfg!(feature = "live-preview") {
        "MainWindow::page-scroll-view"
    } else {
        "GitHubUiComponentsWindow::document"
    };
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing playground document {id}"))
}

#[track_caller]
pub fn reveal(window: &MainWindow, scroll: &ElementHandle, id: &str) -> ElementHandle {
    for _ in 0..80 {
        let origin = scroll.absolute_position();
        let size = scroll.size();
        if let Some(element) = ElementHandle::find_by_element_id(window, id).next() {
            let top = element.absolute_position().y;
            if top >= origin.y && top + element.size().height <= origin.y + size.height {
                return element;
            }
        }
        // Metadata includes clipped mounted controls. Aim real wheel events at
        // the target instead of fixed downward steps that can skip short rows.
        let inner = WindowInner::from_pub(window.window());
        inner.ensure_tree_instantiated();
        let mut target_y = None;
        visit_items(
            &inner.component(),
            TraversalOrder::FrontToBack,
            |tree, _, index, ()| {
                let item = ItemRc::new(tree.clone(), index);
                if item.element_type_names_and_ids(0).is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|(_, element_id)| element_id.as_str() == id)
                }) {
                    target_y = Some(item.map_to_native_window(item.geometry().origin).y);
                    return ItemVisitorResult::Abort;
                }
                ItemVisitorResult::Continue(())
            },
            (),
        );
        let target_y = target_y.unwrap_or_else(|| panic!("unmounted UI element {id}"));
        let delta_y =
            (origin.y + size.height / 3.0 - target_y).clamp(-size.height / 3.0, size.height / 3.0);
        window
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: LogicalPosition::new(
                    origin.x + size.width / 4.0,
                    origin.y + size.height / 2.0,
                ),
                delta_x: 0.0,
                delta_y,
            });
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    }
    panic!("mounted control did not enter its document viewport: {id}")
}
