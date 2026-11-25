mod azure;
mod context;
mod copilot;
mod incremental;
mod publish;
mod render;
mod repository;

use std::{io::Write, num::NonZeroU32};

use clap::{Args, ValueEnum};

use self::repository::Repository;

pub use azure::AzureError;
pub use copilot::CopilotError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(super) enum Effort {
    Adaptive,
    Lite,
    #[default]
    Balanced,
}

#[derive(Args)]
pub(super) struct ReviewArgs {
    /// HTTPS clone URL of the Azure Repos repository (not a pull request URL).
    #[arg(long, value_name = "URL")]
    repo_url: String,
    /// ID of the pull request to review.
    #[arg(long, value_name = "ID")]
    pr: NonZeroU32,
    /// Review depth; adaptive chooses between lite and balanced.
    #[arg(long, value_enum, default_value_t)]
    effort: Effort,
    #[command(flatten)]
    options: ReviewOptions,
}

#[derive(Args)]
struct ReviewOptions {
    /// Exclude paths matching this glob. May be repeated.
    #[arg(long, value_name = "GLOB")]
    exclude: Vec<String>,
    /// Review the latest iteration even if it has already been reviewed.
    #[arg(long)]
    force: bool,
    /// Run the review without posting to Azure DevOps.
    #[arg(long)]
    dry_run: bool,
}

impl ReviewArgs {
    pub(super) fn run(
        self,
        out: &mut impl Write,
        progress: &mut (impl Write + Send),
    ) -> Result<(), super::Error> {
        let repository = Repository::parse(&self.repo_url)
            .map_err(|error| clap::Error::raw(clap::error::ErrorKind::ValueValidation, error))?;
        let exclusions = context::Exclusions::new(&self.options.exclude).map_err(|_| {
            clap::Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "invalid --exclude glob",
            )
        })?;
        self.review(
            (&mut azure::Client::from_env()?, &repository, &exclusions),
            (out, progress),
        )
    }

    fn review(
        &self,
        (client, repository, exclusions): (&mut azure::Client, &Repository, &context::Exclusions),
        streams: (&mut impl Write, &mut (impl Write + Send)),
    ) -> Result<(), super::Error> {
        writeln!(
            streams.1,
            "Loading pull request {} from Azure DevOps",
            self.pr
        )?;
        let snapshot = client.snapshot(repository, self.pr)?;
        let identity = client.identity(repository)?;
        let previous = incremental::PreviousReview::from_threads(&snapshot.threads, identity);
        if previous.includes(snapshot.iteration_id()) && !self.options.force {
            writeln!(
                streams.1,
                "Iteration {} has already been reviewed; skipping",
                snapshot.iteration_id()
            )?;
            return Ok(());
        }
        let target = publish::Target {
            repository,
            pr: self.pr,
            original: &snapshot,
            identity,
            force: self.options.force,
        };
        let context = load_context(client, (&snapshot, exclusions), streams.1)?;
        self.finish((client, &target, &context, &previous), streams)
    }

    fn finish(
        &self,
        (client, target, context, previous): (
            &mut azure::Client,
            &publish::Target<'_>,
            &context::Context,
            &incremental::PreviousReview,
        ),
        (out, progress): (&mut impl Write, &mut (impl Write + Send)),
    ) -> Result<(), super::Error> {
        let (mut review, mode) = copilot::review(
            context,
            (self.effort, &copilot::Credentials::from_env()?),
            progress,
        )?;
        let mut report = publish::Report {
            context,
            review: &mut review,
            effort: (self.effort, mode),
        };
        if self.options.dry_run {
            return publish::preview(
                client,
                (target.original, target.identity, previous),
                (&mut report, out),
            );
        }
        publish::publish(client, target, (&mut report, progress))
    }
}

fn load_context(
    client: &mut azure::Client,
    (snapshot, exclusions): (&azure::Snapshot, &context::Exclusions),
    progress: &mut impl Write,
) -> Result<context::Context, super::Error> {
    let iteration = snapshot.iteration_id();
    let context = context::Context::gather(client, snapshot, exclusions)?;
    let (files, skipped, threads) = context.counts();
    writeln!(
        progress,
        "Loaded iteration {iteration}: {files} changed files, {skipped} skipped, {threads} discussion threads"
    )?;
    Ok(context)
}
