//! Forward showcase feedback to the application's existing controllers/store.
use slint::ComponentHandle;
use usher_ui_components as gallery;

use crate::{MainWindow, NotificationManager, ToastManager};

fn toast(value: gallery::ToastOptions) -> crate::ToastOptions {
    crate::ToastOptions {
        message: value.message,
        duration: value.duration,
        position: value.position,
        icon: value.icon,
        overlay: value.overlay,
        close_on_click: value.close_on_click,
        loading_type: value.loading_type,
        width: value.width,
    }
}

fn notification(value: gallery::NotificationOptions) -> crate::NotificationOptions {
    crate::NotificationOptions {
        title: value.title,
        message: value.message,
        duration: value.duration,
        icon: value.icon,
        actions: value.actions,
        tag: value.tag,
        source: value.source,
        category: value.category,
        component: match value.component {
            gallery::CustomComponentType::None => crate::CustomComponentType::None,
            gallery::CustomComponentType::ActionsNotify => {
                crate::CustomComponentType::ActionsNotify
            }
        },
        native_delivery: match value.native_delivery {
            gallery::NotificationDelivery::WhenUnfocused => {
                crate::NotificationDelivery::WhenUnfocused
            }
            gallery::NotificationDelivery::Never => crate::NotificationDelivery::Never,
            gallery::NotificationDelivery::Always => crate::NotificationDelivery::Always,
        },
    }
}

pub(super) fn bind(window: &MainWindow, page: &gallery::GitHubUiComponentsWindow) {
    // These are event-loop callbacks with weak host handles. Do not create a
    // second Toast/Notification controller or a second durable Inbox store.
    macro_rules! toast_callback {
        ($on:ident, $invoke:ident) => {{
            let weak = window.as_weak();
            page.global::<gallery::ToastManager>().$on(move |options| {
                if let Some(window) = weak.upgrade() {
                    window.global::<ToastManager>().$invoke(toast(options));
                }
            });
        }};
    }
    toast_callback!(on_info, invoke_info);
    toast_callback!(on_success, invoke_success);
    toast_callback!(on_error, invoke_error);
    toast_callback!(on_loading, invoke_loading);
    let weak = window.as_weak();
    page.global::<gallery::ToastManager>()
        .on_show(move |kind, options| {
            if let Some(window) = weak.upgrade() {
                let kind = match kind {
                    gallery::ToastType::Text => crate::ToastType::Text,
                    gallery::ToastType::Success => crate::ToastType::Success,
                    gallery::ToastType::Error => crate::ToastType::Error,
                    gallery::ToastType::Loading => crate::ToastType::Loading,
                };
                window
                    .global::<ToastManager>()
                    .invoke_show(kind, toast(options));
            }
        });
    let weak = window.as_weak();
    page.global::<gallery::ToastManager>().on_dismiss(move || {
        if let Some(window) = weak.upgrade() {
            window.global::<ToastManager>().invoke_dismiss();
        }
    });
    macro_rules! notification_callback {
        ($on:ident, $invoke:ident) => {{
            let weak = window.as_weak();
            page.global::<gallery::NotificationManager>()
                .$on(move |options| {
                    if let Some(window) = weak.upgrade() {
                        window
                            .global::<NotificationManager>()
                            .$invoke(notification(options));
                    }
                });
        }};
    }
    notification_callback!(on_info, invoke_info);
    notification_callback!(on_success, invoke_success);
    notification_callback!(on_warn, invoke_warn);
    notification_callback!(on_error, invoke_error);
    let weak = window.as_weak();
    page.global::<gallery::NotificationManager>()
        .on_show(move |kind, options| {
            if let Some(window) = weak.upgrade() {
                let kind = match kind {
                    gallery::NotificationType::Info => crate::NotificationType::Info,
                    gallery::NotificationType::Success => crate::NotificationType::Success,
                    gallery::NotificationType::Warning => crate::NotificationType::Warning,
                    gallery::NotificationType::Error => crate::NotificationType::Error,
                };
                window
                    .global::<NotificationManager>()
                    .invoke_show(kind, notification(options));
            }
        });
    let weak = window.as_weak();
    page.global::<gallery::NotificationManager>()
        .on_remove_all(move || {
            if let Some(window) = weak.upgrade() {
                window.global::<NotificationManager>().invoke_remove_all();
            }
        });
    let weak = window.as_weak();
    page.global::<gallery::NotificationManager>()
        .on_clear(move || {
            if let Some(window) = weak.upgrade() {
                window.global::<NotificationManager>().invoke_clear();
            }
        });
    let weak = window.as_weak();
    page.global::<gallery::DialogManager>()
        .on_show(move |value| {
            if let Some(window) = weak.upgrade() {
                window
                    .global::<crate::DialogManager>()
                    .invoke_show(crate::DialogOptions {
                        id: value.id,
                        title: value.title,
                        content: value.content,
                        actions: value.actions,
                        default_action: value.default_action,
                        overlay: value.overlay,
                        modal: value.modal,
                        width: value.width,
                        height: value.height,
                    });
            }
        });
}
