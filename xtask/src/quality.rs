mod audit;
mod check;
mod complexity;
mod config;
mod counts;
mod findings;
mod modules;
mod numbers;
mod recursion;
mod runtime_pin;
mod source;

use std::{
    env,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use config::Quality;

fn run(command: Option<&str>, root: &Path, out: &mut impl Write) -> anyhow::Result<()> {
    let quality = Quality::load(&root.join("Cargo.toml"))?;
    match command {
        Some("source") => {
            let findings = audit::workspace(root, &quality)?;
            for finding in &findings {
                writeln!(out, "{finding}")?;
            }
            anyhow::ensure!(
                findings.is_empty(),
                "{} source quality violations",
                findings.len()
            );
        }
        Some("coverage") => writeln!(out, "{}", quality.tools.coverage)?,
        Some("runtime-pin") => runtime_pin::check(root)?,
        Some("duplication") => {
            writeln!(
                out,
                "{} {}",
                quality.tools.duplicate_lines, quality.tools.duplicate_threshold
            )?;
        }
        _ => anyhow::bail!("usage: xtask <source|coverage|duplication|runtime-pin>"),
    }
    Ok(())
}

pub fn main() -> ExitCode {
    let command = env::args().nth(1);
    match run(command.as_deref(), Path::new("."), &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            drop(writeln!(io::stderr().lock(), "{error:#}"));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
