use super::*;
use slint::{Model, ModelRc, VecModel};
use usher_ui_components as gallery;

#[test]
fn factory_uses_host_renderer_and_restores_shell_root() {
    use i_slint_core::{item_tree::ItemTreeRc, window::WindowInner};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    let standalone = GitHubUiComponentsWindow::new().unwrap();
    assert!(!Rc::ptr_eq(
        &host.window_adapter(),
        &WindowInner::from_pub(standalone.window()).window_adapter()
    ));
    drop(standalone);
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    let page = create_page(&window).unwrap();
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    assert!(Rc::ptr_eq(
        &host.window_adapter(),
        &WindowInner::from_pub(page.window()).window_adapter()
    ));
    for _ in 0..20 {
        page.set_route_active(false);
        host.ensure_tree_instantiated();
        page.set_route_active(true);
        host.ensure_tree_instantiated();
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    }
    drop(page);
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
}

#[test]
fn factory_creation_preserves_preview_close_and_native_close_handlers() {
    use i_slint_backend_testing::{AccessibleRole, ElementHandle};
    use slint::platform::{PointerEventButton, WindowEvent};
    use std::{cell::Cell, time::Duration};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    window.set_platform_is_macos(true);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_artifact_preview_kind(crate::ArtifactPreviewKind::PlainText);
    window.set_artifact_preview_plain_text("Preview fixture".into());
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1100.0, 700.0),
    });

    let dismissals = Rc::new(Cell::new(0));
    let captured = dismissals.clone();
    let weak = window.as_weak();
    window.on_artifact_preview_dismiss(move || {
        captured.set(captured.get() + 1);
        weak.upgrade().unwrap().set_artifact_preview_visible(false);
    });
    let native_closes = Rc::new(Cell::new(0));
    let captured = native_closes.clone();
    window.window().on_close_requested(move || {
        captured.set(captured.get() + 1);
        slint::CloseRequestResponse::KeepWindowShown
    });
    let click_close = |expected, phase| {
        window.set_artifact_preview_visible(true);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(350));
        ElementHandle::find_by_accessible_label(&window, "Close preview")
            .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
            .expect("visible Preview close action")
            .mock_single_click(PointerEventButton::Left);
        assert_eq!(dismissals.get(), expected, "{phase}");
        assert!(!window.get_artifact_preview_visible(), "{phase}");
    };

    click_close(1, "host before factory creation");
    let unmounted = create_page(&window).unwrap();
    click_close(2, "shared-window constructor without mounting");
    drop(unmounted);
    click_close(3, "after unmounted factory destruction");
    install(&window);
    for (index, route) in [
        crate::Route::UiComponents,
        crate::Route::Home,
        crate::Route::Messages,
    ]
    .into_iter()
    .enumerate()
    {
        window.set_current_route(route);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(350));
        click_close(index + 4, "after mounted factory route change");
    }
    // Direct dispatch distinguishes callback replacement from pointer routing.
    window.invoke_artifact_preview_dismiss();
    assert_eq!(dismissals.get(), 7);
    window.window().dispatch_event(WindowEvent::CloseRequested);
    assert_eq!(native_closes.get(), 1);
}

#[test]
fn factory_forwards_feedback_once_and_preserves_delivery_policy() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let page = GitHubUiComponentsWindow::new().unwrap();
    feedback::bind(&window, &page);
    let deliveries = Rc::new(RefCell::new(Vec::new()));
    let captured = deliveries.clone();
    window
        .global::<crate::NotificationManager>()
        .on_show(move |kind, options| {
            captured.borrow_mut().push((kind, options));
        });
    page.global::<gallery::NotificationManager>().invoke_show(
        gallery::NotificationType::Warning,
        gallery::NotificationOptions {
            title: "title".into(),
            message: "message".into(),
            duration: 4210,
            icon: "icon".into(),
            actions: ModelRc::new(VecModel::from(vec!["action".into()])),
            tag: "tag".into(),
            source: "source".into(),
            category: "category".into(),
            component: gallery::CustomComponentType::ActionsNotify,
            native_delivery: gallery::NotificationDelivery::Never,
        },
    );
    let deliveries = deliveries.borrow();
    assert_eq!(
        deliveries.len(),
        1,
        "one factory request must have one host delivery"
    );
    let (kind, options) = &deliveries[0];
    assert_eq!(*kind, crate::NotificationType::Warning);
    assert_eq!(options.title, "title");
    assert_eq!(options.message, "message");
    assert_eq!(options.duration, 4210);
    assert_eq!(options.icon, "icon");
    assert_eq!(options.actions.row_data(0).unwrap(), "action");
    assert_eq!(options.tag, "tag");
    assert_eq!(options.source, "source");
    assert_eq!(options.category, "category");
    assert_eq!(options.component, crate::CustomComponentType::ActionsNotify);
    assert_eq!(options.native_delivery, crate::NotificationDelivery::Never);
}

#[test]
fn factory_uses_host_geometry_theme_and_weak_callback_lifetime() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let page = GitHubUiComponentsWindow::new().unwrap();
    window.global::<AppState>().set_effective_dark_mode(true);
    window.global::<AppState>().set_theme_mode(2);
    window.set_current_route(crate::Route::UiComponents);
    window.set_application_font_family("fixture-font".into());
    let host = window.global::<crate::UiComponentsBridge>();
    host.set_window_bounds(crate::FloatingOverlayRect {
        x: 0.0,
        y: 0.0,
        width: 1100.0,
        height: 700.0,
    });
    host.set_viewport(crate::FloatingOverlayRect {
        x: 210.0,
        y: 46.0,
        width: 890.0,
        height: 618.0,
    });
    host.set_top_padding(16.0);

    sync(&window, &page);
    assert_eq!(
        page.get_content_width(),
        window
            .global::<crate::UiComponentsBridge>()
            .get_viewport()
            .width
    );
    assert_eq!(
        page.get_content_y(),
        window
            .global::<crate::UiComponentsBridge>()
            .get_viewport()
            .y
    );
    assert_eq!(
        page.get_window_height(),
        window
            .global::<crate::UiComponentsBridge>()
            .get_window_bounds()
            .height
    );
    assert_eq!(page.get_content_width(), 890.0);
    assert_eq!(page.get_content_y(), 46.0);
    assert_eq!(page.get_window_height(), 700.0);
    assert_eq!(page.get_content_top_padding(), 16.0);
    assert_eq!(page.get_application_font_family(), "fixture-font");
    assert!(page.global::<GalleryAppState>().get_effective_dark_mode());
    assert_eq!(page.global::<GalleryAppState>().get_theme_mode(), 2);
    feedback::bind(&window, &page);
    drop(window);
    // A delayed factory control cannot keep or dereference a destroyed host.
    page.global::<gallery::ToastManager>()
        .invoke_info(Default::default());
    page.global::<gallery::NotificationManager>().invoke_clear();
    page.global::<gallery::DialogManager>()
        .invoke_show(Default::default());
}

#[test]
fn factory_keeps_toast_and_dialog_options_on_the_host() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let page = GitHubUiComponentsWindow::new().unwrap();
    feedback::bind(&window, &page);
    let toast_calls = Rc::new(RefCell::new(Vec::new()));
    let captured = toast_calls.clone();
    window
        .global::<crate::ToastManager>()
        .on_show(move |kind, options| {
            captured.borrow_mut().push((kind, options));
        });
    page.global::<gallery::ToastManager>().invoke_show(
        gallery::ToastType::Loading,
        gallery::ToastOptions {
            message: "loading".into(),
            duration: 1234,
            position: "bottom".into(),
            icon: "icon".into(),
            overlay: true,
            close_on_click: true,
            loading_type: "circular".into(),
            width: "52%".into(),
        },
    );
    let calls = toast_calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, crate::ToastType::Loading);
    assert_eq!(
        calls[0].1,
        crate::ToastOptions {
            message: "loading".into(),
            duration: 1234,
            position: "bottom".into(),
            icon: "icon".into(),
            overlay: true,
            close_on_click: true,
            loading_type: "circular".into(),
            width: "52%".into(),
        }
    );
    let dialog_calls = Rc::new(RefCell::new(Vec::new()));
    let captured = dialog_calls.clone();
    window
        .global::<crate::DialogManager>()
        .on_show(move |options| {
            captured.borrow_mut().push(options);
        });
    page.global::<gallery::DialogManager>()
        .invoke_show(gallery::DialogOptions {
            id: "fixture".into(),
            title: "title".into(),
            content: "content".into(),
            actions: ModelRc::new(VecModel::from(vec!["cancel".into()])),
            default_action: "confirm".into(),
            modal: true,
            overlay: true,
            width: "52%".into(),
            height: "280px".into(),
        });
    let calls = dialog_calls.borrow();
    assert_eq!(calls.len(), 1);
    let options = &calls[0];
    assert_eq!(options.id, "fixture");
    assert_eq!(options.title, "title");
    assert_eq!(options.content, "content");
    assert_eq!(options.default_action, "confirm");
    assert_eq!(options.actions.row_data(0).unwrap(), "cancel");
    assert!(options.modal && options.overlay);
    assert_eq!(options.width, "52%");
    assert_eq!(options.height, "280px");
}
