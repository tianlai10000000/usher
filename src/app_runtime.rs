use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};

lazy_static::lazy_static! {
    static ref SHARED_RUNTIME: Arc<Runtime> = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .on_thread_start(|| crate::logger::tracing_logger::install_thread_dispatch())
            .build()
            .expect("Failed to create shared Tokio runtime")
    );
}

pub fn shared_runtime() -> Arc<Runtime> {
    SHARED_RUNTIME.clone()
}
