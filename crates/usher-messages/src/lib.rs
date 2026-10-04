//! Application message center and independently compiled message-list UI.
//!
//! The domain service has no dependency on Usher's root window. Consumers
//! publish through [`MessageCenter`], while presentation bridges subscribe to
//! immutable snapshots. Native delivery is delegated to `notify-rust`.

mod center;
mod desktop;
mod model;
mod native_worker;
mod paging;
mod storage;

pub use center::{MessageCenter, MessageCenterError, MessageSnapshot, Subscription};
pub use desktop::{DesktopNotifier, NoopDesktopNotifier, NotifyRustDesktopNotifier};
pub use model::{Message, MessageDraft, MessageSeverity, NativeDelivery, categories};
pub use paging::{MESSAGE_PAGE_SIZE, MessagePage, MessageReadFilter};

slint::include_modules!();
