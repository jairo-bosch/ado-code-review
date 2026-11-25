//! CLI entry point and errors for Azure DevOps code review.

mod app;

pub use app::{AzureError, CopilotError, Error, run, run_with_progress};
