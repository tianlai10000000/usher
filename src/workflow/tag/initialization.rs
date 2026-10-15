//! Bounded, cancellable Tag inspection with a fixed per-command deadline.
use super::command_output::{CommandFailureKind, run_command_with_deadline};
use super::initialization_error::LocalTagReadError;
use super::version::{RemoteTagParser, parse_semver_tag};
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
    commands.push(if remove {
        vec![
            "git".into(),
            "-C".into(),
            repo.into(),
            "ls-remote".into(),
            "--tags".into(),
            "--refs".into(),
            "origin".into(),
        ]
    } else {
        vec![
            "git".into(),
            "-C".into(),
            repo.into(),
            "tag".into(),
            "--list".into(),
            "--sort=-version:refname".into(),
        ]
    });
    let mut parser = TagParser::new(remove);
    for (index, command) in commands.into_iter().enumerate() {
        let consume = !remove || index == 1;
        run_command_with_deadline(
            command,
            cancel,
            |_| {},
            |text| if consume { parser.push(text) } else { Ok(()) },
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
struct TagParser {
    remote: bool,
    remote_parser: RemoteTagParser,
    pending: String,
    latest: Option<String>,
}
impl TagParser {
    fn new(remote: bool) -> Self {
        Self {
            remote,
            remote_parser: RemoteTagParser::default(),
            pending: String::new(),
            latest: None,
        }
    }
    fn push(&mut self, text: &str) -> Result<(), String> {
        if self.remote {
            return self
                .remote_parser
                .push(text)
                .map_err(|error| error.to_string());
        }
        for segment in text.split_inclusive('\n') {
            if self.pending.len() + segment.len() > 8192 + usize::from(segment.ends_with('\n')) {
                return Err("local Tag line exceeded 8 KiB".into());
            }
            self.pending.push_str(segment);
            if self.pending.ends_with('\n') {
                self.observe();
            }
        }
        Ok(())
    }
    fn observe(&mut self) {
        let line = std::mem::take(&mut self.pending);
        let tag = line.trim();
        if self.latest.is_none() && parse_semver_tag(tag).is_some() {
            self.latest = Some(tag.into());
        }
    }
    fn finish(mut self) -> Result<String, String> {
        if self.remote {
            return self
                .remote_parser
                .finish()
                .map_err(|error| error.to_string());
        }
        self.observe();
        Ok(self.latest.unwrap_or_else(|| "v0.0.0".into()))
    }
}

#[cfg(test)]
#[path = "initialization_tests.rs"]
mod tests;
