use std::io::Write;

use clap::{Parser, Subcommand};

use super::{Error, review::ReviewArgs};

#[derive(Parser)]
#[command(version, about = "Review Azure DevOps pull requests")]
pub(super) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Review an Azure DevOps pull request.
    Review(ReviewArgs),
}

impl Cli {
    pub(super) fn run(
        self,
        out: &mut impl Write,
        progress: &mut (impl Write + Send),
    ) -> Result<(), Error> {
        let Command::Review(args) = self.command;
        args.run(out, progress)
    }
}
