mod cli;
mod review;

use std::{ffi::OsString, io::Write, process::ExitCode};

use clap::{Parser, error::ErrorKind};

pub use review::{AzureError, CopilotError};

/// An error encountered while running the CLI.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Usage(#[from] clap::Error),
    #[error(transparent)]
    Azure(#[from] AzureError),
    #[error(transparent)]
    Copilot(#[from] CopilotError),
    #[error("cannot write output: {0}")]
    Output(#[from] std::io::Error),
}

impl Error {
    /// Returns the process exit code for this error.
    pub fn exit_code(&self) -> ExitCode {
        const USAGE_ERROR: u8 = 2;
        match self {
            Self::Usage(_) => ExitCode::from(USAGE_ERROR),
            _ => ExitCode::FAILURE,
        }
    }
}

/// Parses the arguments (including the executable name) and writes the result or help.
///
/// # Errors
///
/// Returns an error for invalid arguments, command failures, or failed writes.
pub fn run(
    args: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    out: &mut impl Write,
) -> Result<(), Error> {
    run_with_progress(args, out, &mut std::io::sink())
}

/// Parses the arguments and writes progress and command output to separate streams.
///
/// # Errors
///
/// Returns an error for invalid arguments, command failures, or failed writes.
pub fn run_with_progress(
    args: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    out: &mut impl Write,
    progress: &mut (impl Write + Send),
) -> Result<(), Error> {
    match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli.run(out, progress)?,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            write!(out, "{error}")?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
