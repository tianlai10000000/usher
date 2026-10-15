//! Semantic download progress retained independently from its UI locale.
use std::cell::RefCell;

use crate::{MainWindow, UpdateLogic};
use slint::ComponentHandle;

#[derive(Clone, Debug, Default)]
pub enum DownloadStatus {
    #[default]
    None,
    Preparing,
    PreparingDownload,
    Connecting,
    FetchingRelease,
    Release(String),
    Bytes {
        downloaded: u64,
        total: u64,
    },
    DownloadInstalling,
    Extracting,
    Installing,
    Complete,
    Restarting,
    RestartingApplication,
}

impl DownloadStatus {
    pub(crate) fn resolve(&self) -> String {
        let key = match self {
            Self::None => return String::new(),
            Self::Preparing => "update.preparing",
            Self::PreparingDownload => "update.preparing_download",
            Self::Connecting => "update.connecting",
            Self::FetchingRelease => "update.fetching_release",
            Self::Release(version) => {
                return crate::i18n::tr_args("update.downloading_release", &[("version", version)]);
            }
            Self::Bytes { downloaded, total } => {
                let downloaded_mb = format!("{:.1}", *downloaded as f64 / 1_048_576.0);
                if *total == 0 {
                    return crate::i18n::tr_args(
                        "update.downloading_amount",
                        &[("downloaded_mb", &downloaded_mb)],
                    );
                }
                let total_mb = format!("{:.1}", *total as f64 / 1_048_576.0);
                return crate::i18n::tr_args(
                    "update.downloading_progress",
                    &[("downloaded_mb", &downloaded_mb), ("total_mb", &total_mb)],
                );
            }
            Self::DownloadInstalling => "update.download_installing",
            Self::Extracting => "update.extracting",
            Self::Installing => "update.installing",
            Self::Complete => "update.complete",
            Self::Restarting => "update.complete_restarting",
            Self::RestartingApplication => "update.restarting",
        };
        crate::i18n::tr(key).into_owned()
    }
}

thread_local! {
    static PRESENTED_STATUS: RefCell<Option<(slint::Weak<MainWindow>, DownloadStatus)>> = const { RefCell::new(None) };
}

pub(crate) fn present(ui: &MainWindow, status: DownloadStatus) {
    ui.global::<UpdateLogic>()
        .set_download_status(status.resolve().into());
    PRESENTED_STATUS.with(|state| *state.borrow_mut() = Some((ui.as_weak(), status)));
}

pub(crate) fn sync_language(ui: &MainWindow) {
    let status = PRESENTED_STATUS.with(|state| {
        state.borrow().as_ref().and_then(|(weak, status)| {
            weak.upgrade()
                .filter(|window| std::ptr::eq(window.window(), ui.window()))
                .map(|_| status.clone())
        })
    });
    if let Some(status) = status {
        ui.global::<UpdateLogic>()
            .set_download_status(status.resolve().into());
    }
}
