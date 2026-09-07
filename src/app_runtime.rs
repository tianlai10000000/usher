use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};

const PRODUCTION_WORKER_THREADS: usize = 2;

lazy_static::lazy_static! {
    static ref SHARED_RUNTIME: Arc<Runtime> = Arc::new({
        let worker_threads = selected_worker_threads(
            std::env::var("USHER_TOKIO_WORKER_THREADS").ok().as_deref(),
        );
        let mut builder = Builder::new_multi_thread();
        if let Some(worker_threads) = worker_threads {
            builder.worker_threads(worker_threads);
        }
        log::info!(
            "[Runtime] Shared Tokio worker threads: {}",
            worker_threads.map_or_else(|| "default".to_string(), |value| value.to_string())
        );
        builder
            .enable_all()
            .on_thread_start(crate::logger::tracing_logger::install_thread_dispatch)
            .build()
            .expect("Failed to create shared Tokio runtime")
    });
}

fn selected_worker_threads(value: Option<&str>) -> Option<usize> {
    match value {
        None => Some(PRODUCTION_WORKER_THREADS),
        Some("default") => None,
        Some(value) => value.parse::<usize>().ok().filter(|value| *value > 0),
    }
}

pub fn shared_runtime() -> Arc<Runtime> {
    SHARED_RUNTIME.clone()
}

#[cfg(test)]
mod tests {
    use super::selected_worker_threads;

    #[test]
    fn worker_override_accepts_only_positive_numbers() {
        assert_eq!(selected_worker_threads(None), Some(2));
        assert_eq!(selected_worker_threads(Some("default")), None);
        assert_eq!(selected_worker_threads(Some("0")), None);
        assert_eq!(selected_worker_threads(Some("invalid")), None);
        assert_eq!(selected_worker_threads(Some("2")), Some(2));
    }
}

mod latest_ui;
pub(crate) use latest_ui::latest_ui;
