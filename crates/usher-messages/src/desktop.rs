use crate::Message;
#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
use crate::MessageSeverity;

/// Port for OS-level delivery. Tests and unsupported environments can swap in
/// a no-op without changing message-center behavior.
pub trait DesktopNotifier: Send + Sync + 'static {
    fn show(&self, message: &Message) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct NotifyRustDesktopNotifier;

impl DesktopNotifier for NotifyRustDesktopNotifier {
    fn show(&self, message: &Message) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            static APPLICATION_IDENTITY: std::sync::Once = std::sync::Once::new();
            APPLICATION_IDENTITY.call_once(|| {
                if let Err(error) = notify_rust::set_application("com.usher.app") {
                    // Unbundled `cargo run` builds may not be registered with
                    // LaunchServices. Delivery still gets a best-effort chance;
                    // packaged builds resolve this bundle identifier normally.
                    log::debug!("[Messages] macOS notification identity unavailable: {error}");
                }
            });
        }

        let mut notification = notify_rust::Notification::new();
        notification
            .appname("Usher")
            .summary(&message.title)
            .body(&message.body)
            .auto_icon();

        #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
        {
            use notify_rust::Urgency;
            notification.urgency(match message.severity {
                MessageSeverity::Info | MessageSeverity::Success => Urgency::Normal,
                MessageSeverity::Warning | MessageSeverity::Error => Urgency::Critical,
            });
        }

        notification
            .show()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Default)]
pub struct NoopDesktopNotifier;

impl DesktopNotifier for NoopDesktopNotifier {
    fn show(&self, _message: &Message) -> Result<(), String> {
        Ok(())
    }
}
