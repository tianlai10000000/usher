//! Bounded, cancellable Tag inspection with a fixed per-command deadline.
use super::command_output::{CommandFailureKind, run_command_with_deadline};
use super::initialization_error::LocalTagReadError;
use super::version::RemoteTagParser;
use crate::i18n::LocalizedText;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
pub(super) async fn inspect(
    repo: &str,
    remove: bool,
    cancel: &Arc<AtomicBool>,
) -> Result<String, LocalizedText> {
    if repo.trim().is_empty() {
        return Ok("v0.0.0".into());
    }
    if !tokio::fs::try_exists(repo).await.unwrap_or(false) {
        return Err(LocalTagReadError::RepositoryMissing.localized());
    }
    let mut commands = Vec::new();
    if remove {
        commands.push(vec![
            "git".into(),
            "-C".into(),
            repo.into(),
            "fetch".into(),
            "origin".into(),
            "--tags".into(),
            "--force".into(),
        ]);
    }
    commands.push(vec![
        "git".into(),
        "-C".into(),
        repo.into(),
        "ls-remote".into(),
        "--tags".into(),
        "--refs".into(),
        "origin".into(),
    ]);
    let mut parser = RemoteTagParser::default();
    for (index, command) in commands.into_iter().enumerate() {
        let consume = !remove || index == 1;
        run_command_with_deadline(
            command,
            cancel,
            |_| {},
            |text| {
                if consume {
                    parser.push(text).map_err(|error| error.to_string())
                } else {
                    Ok(())
                }
            },
            Some(TIMEOUT),
        )
        .await
        .map_err(|error| {
            log::warn!("[Workflow][Tag] Initialization failed: {}", error.log);
            if error.kind == CommandFailureKind::TimedOut {
                return LocalizedText::message("tag.initialization_timeout");
            }
            if let Some(kind) = error.spawn_kind {
                return LocalTagReadError::from_spawn(kind.into()).localized();
            }
            LocalTagReadError::classify_failed_output(
                error.exit_status.unwrap_or_else(|| "Git".into()),
                &error.log,
            )
            .localized()
        })?;
        if cancel.load(Ordering::Acquire) {
            return Err(LocalizedText::message("tag.stop_requested"));
        }
    }
    parser
        .finish()
        .map_err(|_| LocalizedText::message("tag.invalid_version"))
}

#[cfg(test)]
#[path = "initialization_tests.rs"]
mod tests;
