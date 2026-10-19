//! Event-loop pointer delivery to independent, full-window UI global graphs.
use crate::MainWindow;
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};

struct Observer {
    host: slint::Weak<MainWindow>,
    alive: Box<dyn Fn() -> bool>,
    publish: Rc<dyn Fn(f32, f32)>,
}

thread_local! {
    static OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
}

/// Register once when creating an independent factory root. Coordinates are
/// logical window coordinates; the factory must occupy the full window at 0,0.
/// The callback owns route-activity policy, and neither host nor root is retained.
pub(crate) fn register_pointer_root<C: ComponentHandle + 'static>(
    window: &MainWindow,
    root: &C,
    publish: impl Fn(&C, f32, f32) + 'static,
) {
    let owner = root.as_weak();
    let alive = owner.clone();
    OBSERVERS.with(|observers| {
        let mut observers = observers.borrow_mut();
        observers.retain(|entry| entry.host.upgrade().is_some() && (entry.alive)());
        observers.push(Observer {
            host: window.as_weak(),
            alive: Box::new(move || alive.upgrade().is_some()),
            publish: Rc::new(move |x, y| {
                if let Some(root) = owner.upgrade() {
                    publish(&root, x, y);
                }
            }),
        });
    });
}

pub(super) fn publish_pointer_press(window: &MainWindow, x: f32, y: f32) {
    window
        .global::<crate::TransientOverlayState>()
        .invoke_pointer_pressed(x, y);
    // Call outside the registry borrow: UI changes may construct another root.
    let callbacks = OBSERVERS.with(|observers| {
        let mut observers = observers.borrow_mut();
        observers.retain(|entry| entry.host.upgrade().is_some() && (entry.alive)());
        observers
            .iter()
            .filter(|entry| {
                entry
                    .host
                    .upgrade()
                    .is_some_and(|host| std::ptr::eq(host.window(), window.window()))
            })
            .map(|entry| entry.publish.clone())
            .collect::<Vec<_>>()
    });
    for publish in callbacks {
        publish(x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    slint::slint! { export component ObserverRoot inherits Window {} }

    #[test]
    fn factory_delivery_is_host_scoped_active_and_weak() {
        i_slint_backend_testing::init_no_event_loop();
        let host = MainWindow::new().unwrap();
        let other = MainWindow::new().unwrap();
        let root = ObserverRoot::new().unwrap();
        let weak = root.as_weak();
        let active = Rc::new(Cell::new(true));
        let calls = Rc::new(Cell::new(0));
        let (enabled, count) = (active.clone(), calls.clone());
        register_pointer_root(&host, &root, move |_, x, y| {
            if enabled.get() {
                assert_eq!((x, y), (123.0, 456.0));
                count.set(count.get() + 1);
            }
        });
        publish_pointer_press(&other, 123.0, 456.0);
        assert_eq!(calls.get(), 0);
        publish_pointer_press(&host, 123.0, 456.0);
        assert_eq!(calls.get(), 1);
        active.set(false);
        publish_pointer_press(&host, 123.0, 456.0);
        assert_eq!(calls.get(), 1);
        drop(root);
        assert!(
            weak.upgrade().is_none(),
            "subscription must not retain its root"
        );
        publish_pointer_press(&host, 123.0, 456.0);
        assert_eq!(calls.get(), 1);
    }
}
