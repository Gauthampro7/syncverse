//! Strongly typed error enums for UI and OSD rendering.

use thiserror::Error;

/// Error variants encountered during UI state and rendering operations.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum UiError {
    /// Framebuffer or texture allocation exceeds memory budget.
    #[error("memory budget exceeded: requested {requested_mb:.2} MB > ceiling {ceiling_mb:.2} MB")]
    BudgetExceeded {
        /// Requested memory in MB.
        requested_mb: f64,
        /// Ceiling budget in MB.
        ceiling_mb: f64,
    },

    /// Invalid command or scrubber event sequence.
    #[error("invalid UI event: {0}")]
    InvalidEvent(String),
}
