use std::path::PathBuf;

use directories::ProjectDirs;

const BENCHMARK_OFFLINE_ENV: &str = "USHER_BENCHMARK_OFFLINE";
const BENCHMARK_STATE_ROOT_ENV: &str = "USHER_BENCHMARK_STATE_ROOT";

pub(crate) fn benchmark_offline_mode() -> bool {
    std::env::var(BENCHMARK_OFFLINE_ENV).as_deref() == Ok("1")
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    benchmark_state_dir().or_else(|| {
        ProjectDirs::from("com", "usher", "app").map(|dirs| dirs.config_dir().to_path_buf())
    })
}

pub(crate) fn data_local_dir() -> Option<PathBuf> {
    benchmark_state_dir().or_else(|| {
        ProjectDirs::from("com", "usher", "app").map(|dirs| dirs.data_local_dir().to_path_buf())
    })
}

pub(crate) fn message_store_path() -> Option<PathBuf> {
    data_local_dir().map(|directory| directory.join("messages/messages.json"))
}

fn benchmark_state_dir() -> Option<PathBuf> {
    if !benchmark_offline_mode() {
        return None;
    }
    let root = PathBuf::from(std::env::var_os(BENCHMARK_STATE_ROOT_ENV)?);
    if !root.is_absolute() {
        return None;
    }
    let variant = environment_segment("USHER_AI_REVIEW_RESPONSE_VARIANT", "default");
    let run = environment_segment("USHER_MEMORY_BENCHMARK_RUN", "0");
    Some(root.join(variant).join(format!("run-{run}")))
}

fn environment_segment(name: &str, fallback: &'static str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 80
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::environment_segment;

    #[test]
    fn benchmark_path_segments_reject_path_syntax() {
        // The helper's fallback behavior is covered without mutating process-wide
        // environment variables in the parallel test runner.
        assert_eq!(
            environment_segment("USHER_TEST_UNSET_PATH_SEGMENT", "safe"),
            "safe"
        );
    }
}
