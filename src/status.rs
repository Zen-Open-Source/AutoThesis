//! Strongly-typed status enums with string-compatible serialization.
//!
//! Callers can continue to pass `&str` through database and JSON boundaries
//! while internal logic gets compile-time exhaustiveness checks.

use std::fmt;

/// Lifecycle states for a research `Run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// A run is "terminal" if it will not change state again.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// A run is "in-flight" if it's scheduled to do or is currently doing work.
    pub fn is_in_flight(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        for s in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert_eq!(RunStatus::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(RunStatus::parse("no-such-status"), None);
    }

    #[test]
    fn terminal_and_in_flight_are_disjoint() {
        for s in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert_ne!(s.is_terminal(), s.is_in_flight());
        }
    }
}
