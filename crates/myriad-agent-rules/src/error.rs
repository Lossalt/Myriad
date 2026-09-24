//! Step failure types: what a failed capability call reports, and whether it
//! may already have taken effect.

/// Prefix a callee puts on an error when it cannot tell whether the call took
/// effect (a lost cross-process mutation response). Producers outside this
/// crate (MCP, web control) spell it the same; a test pins them together.
pub const OUTCOME_UNKNOWN_PREFIX: &str = "Execution outcome is unknown:";

/// What a failed step may have done before it failed. Decided once, where the
/// failure is observed, and carried with the error from there on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Refused or failed before taking effect.
    NotApplied,
    /// The call was sent but no response came back (timeout, dropped
    /// connection, cancelled mid-call): the effect may have landed.
    ResponseLost,
    /// The call returned success, then its result was rejected.
    Applied,
    /// The callee itself declared the outcome unknown.
    Unknown,
}

impl StepOutcome {
    pub fn may_have_applied(self) -> bool {
        self != Self::NotApplied
    }
}

/// A failed step: the message shown to the model and the user, and what the
/// failure may have done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    pub message: String,
    pub outcome: StepOutcome,
}

impl StepError {
    pub fn not_applied(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: StepOutcome::NotApplied,
        }
    }

    pub fn response_lost(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: StepOutcome::ResponseLost,
        }
    }

    pub fn applied(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: StepOutcome::Applied,
        }
    }

    /// A capability handler's error. Handlers report failures as text, so this
    /// is the one place the text is read for what it says about the outcome.
    pub fn from_handler(message: String) -> Self {
        let outcome = if message.starts_with(OUTCOME_UNKNOWN_PREFIX) {
            StepOutcome::Unknown
        } else if reports_lost_response(&message) {
            StepOutcome::ResponseLost
        } else {
            StepOutcome::NotApplied
        };
        Self { message, outcome }
    }
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<StepError> for String {
    fn from(error: StepError) -> Self {
        error.message
    }
}

fn reports_lost_response(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("connection closed")
}

#[cfg(test)]
mod tests {
    use super::{StepError, StepOutcome};

    #[test]
    fn handler_errors_carry_what_the_failure_may_have_done() {
        let lost = StepError::from_handler("The step timed out".into());
        assert_eq!(lost.outcome, StepOutcome::ResponseLost);
        assert!(lost.outcome.may_have_applied());
        let unknown = StepError::from_handler("Execution outcome is unknown: lost".into());
        assert_eq!(unknown.outcome, StepOutcome::Unknown);
        assert!(unknown.outcome.may_have_applied());
        let refused = StepError::from_handler("HTTP 503 service unavailable".into());
        assert_eq!(refused.outcome, StepOutcome::NotApplied);
        assert!(!refused.outcome.may_have_applied());
        assert!(StepError::applied("bad output").outcome.may_have_applied());
    }
}
