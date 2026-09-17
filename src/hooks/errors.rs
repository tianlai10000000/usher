//! Stable host/validation categories. Technical diagnostics stay in journals.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HookFailureKind {
    Journal,
    Preparation,
    WorkingDirectory,
    InterpreterStart,
    ProcessTree,
    Wait,
    OutputDrain,
    InvalidDefinition,
    #[serde(other)]
    Unknown,
}

pub(super) struct HookHostError {
    pub kind: HookFailureKind,
    pub diagnostic: String,
}

impl HookHostError {
    pub fn new(kind: HookFailureKind, diagnostic: impl std::fmt::Display) -> Self {
        Self {
            kind,
            diagnostic: diagnostic.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HookValidationError {
    InvalidId,
    NameRequired,
    ScriptEmpty,
    ScriptTooLarge,
    TimeoutRange,
    FieldTooLong,
    InterpreterPath,
    TimeoutInteger,
}

impl std::fmt::Display for HookValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidId => "Invalid Hook ID.",
            Self::NameRequired => "Hook name is required.",
            Self::ScriptEmpty => "Hook script is empty.",
            Self::ScriptTooLarge => "Hook script exceeds 64 KiB.",
            Self::TimeoutRange => "Timeout must be between 1 and 3600 seconds.",
            Self::FieldTooLong => "Hook configuration field is too long.",
            Self::InterpreterPath => "Interpreter must be one executable path.",
            Self::TimeoutInteger => "Timeout must be a whole number of seconds.",
        })
    }
}

impl From<HookValidationError> for String {
    fn from(error: HookValidationError) -> String {
        error.to_string()
    }
}
