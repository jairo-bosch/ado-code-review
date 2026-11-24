//! CLI entry point and errors for the calculator.

mod calculator;
mod cli;

use std::{ffi::OsString, io::Write, process::ExitCode};

use clap::{Parser, error::ErrorKind};

/// An error encountered while running the calculator.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Usage(#[from] clap::Error),
    #[error("operands must be finite")]
    NonFiniteOperand,
    #[error("division by zero")]
    DivisionByZero,
    #[error("result is not finite")]
    NonFiniteResult,
    #[error("cannot write output: {0}")]
    Output(#[from] std::io::Error),
}

impl Error {
    /// Returns the process exit code for this error.
    #[must_use]
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
/// Returns an error for invalid arguments, invalid arithmetic, or a failed write.
pub fn run(
    args: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    out: &mut impl Write,
) -> Result<(), Error> {
    match cli::Cli::try_parse_from(args) {
        Ok(cli) => writeln!(out, "{}", cli.calculate()?)?,
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
