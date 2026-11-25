mod changes;
mod credentials;
mod http;
mod model;
#[cfg(test)]
mod tests;
mod writes;

use std::num::NonZeroU32;

use serde::de::DeserializeOwned;

use super::repository::Repository;
use model::{Collection, ConnectionData, Iteration, PullRequest};

pub(super) use changes::FileChange;
pub(super) use model::{Details, Thread};
pub(super) use writes::Inline;

/// An Azure DevOps operation failed.
#[derive(Debug, thiserror::Error)]
pub enum AzureError {
    #[error("missing or invalid environment variable: AZURE_TOKEN")]
    Token,
    #[error("could not construct Azure DevOps REST endpoint")]
    Endpoint,
    #[error("Azure DevOps returned HTTP {0}")]
    Status(u16),
    #[error("could not connect to Azure DevOps")]
    Transport,
    #[error("Azure DevOps returned an invalid response")]
    Response,
    #[error("the pull request belongs to a different Azure DevOps repository")]
    WrongRepository,
    #[error("Azure DevOps returned no pull request iterations")]
    MissingIteration,
    #[error("Azure DevOps returned an invalid continuation for pull request changes")]
    InvalidChangesPage,
    #[error("target branch review instructions are invalid or exceed the context budget")]
    Instructions,
    #[error("the existing review summary was changed or is ambiguous; nothing was published")]
    ProtectedSummary,
}

pub(super) struct Client(http::Transport);

pub(super) struct Snapshot {
    repository: Repository,
    pr: PullRequestScope,
    iteration: Iteration,
    pub(super) details: Details,
    pub(super) threads: Vec<Thread>,
}

struct PullRequestScope {
    id: NonZeroU32,
    web: url::Url,
    history: Vec<Iteration>,
}

impl Snapshot {
    pub(super) const fn iteration_id(&self) -> NonZeroU32 {
        self.iteration.id
    }

    pub(super) fn discussion_url(&self, thread: NonZeroU32) -> url::Url {
        let mut url = self.pr.web.clone();
        url.query_pairs_mut()
            .append_pair("discussionId", &thread.to_string());
        url
    }

    pub(super) fn matches_iteration(&self, other: &Self) -> bool {
        self.iteration == other.iteration
    }

    pub(super) fn is_current(&self, client: &mut Client) -> Result<bool, AzureError> {
        let iterations: Collection<Iteration> = client.get(
            &self.repository,
            &["pullRequests", &self.pr.id.to_string(), "iterations"],
        )?;
        let latest = iterations
            .value
            .into_iter()
            .max_by_key(|iteration| iteration.id)
            .ok_or(AzureError::MissingIteration)?;
        Ok(latest == self.iteration)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Revision {
    Base,
    Head,
    Target,
}

impl Client {
    pub(super) fn from_env() -> Result<Self, AzureError> {
        Ok(Self(http::Transport::new(&credentials::from_env()?)?))
    }

    pub(super) fn identity(&mut self, repository: &Repository) -> Result<uuid::Uuid, AzureError> {
        let endpoint = repository
            .connection_data()
            .map_err(|_| AzureError::Endpoint)?;
        let connection: ConnectionData = self.0.get(&endpoint)?;
        let id = connection.authenticated_user.id;
        if id.is_nil() {
            return Err(AzureError::Response);
        }
        Ok(id)
    }

    pub(super) fn snapshot(
        &mut self,
        repository: &Repository,
        pull_request_id: NonZeroU32,
    ) -> Result<Snapshot, AzureError> {
        let id = pull_request_id.to_string();
        let pr = self.pull_request(repository, pull_request_id)?;
        let scope = repository.with_ids(pr.repository.project.id, pr.repository.id);
        let mut iterations: Collection<Iteration> =
            self.get(&scope, &["pullRequests", &id, "iterations"])?;
        iterations
            .value
            .sort_unstable_by_key(|iteration| iteration.id);
        let iteration = iterations.value.pop().ok_or(AzureError::MissingIteration)?;
        let threads = self.tracked_threads(&scope, (&id, iteration.id))?;
        Ok(Snapshot {
            repository: scope,
            pr: PullRequestScope {
                id: pull_request_id,
                history: iterations.value,
                web: repository
                    .pull_request_web_url(
                        (&pr.repository.project.name, &pr.repository.name),
                        pull_request_id,
                    )
                    .map_err(|_| AzureError::Endpoint)?,
            },
            iteration,
            details: pr.details,
            threads,
        })
    }

    fn tracked_threads(
        &mut self,
        scope: &Repository,
        (id, iteration): (&str, NonZeroU32),
    ) -> Result<Vec<Thread>, AzureError> {
        let mut endpoint = scope
            .git_api(&["pullRequests", id, "threads"])
            .map_err(|_| AzureError::Endpoint)?;
        endpoint
            .query_pairs_mut()
            .append_pair("$iteration", &iteration.to_string());
        let mut threads: Collection<Thread> = self.0.get(&endpoint)?;
        threads.value.retain_mut(Thread::keep_live);
        Ok(threads.value)
    }

    fn pull_request(
        &mut self,
        repository: &Repository,
        pr: NonZeroU32,
    ) -> Result<PullRequest, AzureError> {
        let endpoint = repository
            .api(["pullRequests", pr.to_string().as_str()])
            .map_err(|_| AzureError::Endpoint)?;
        let pull_request: PullRequest = self.0.get(&endpoint)?;
        if pull_request.id != pr {
            return Err(AzureError::Response);
        }
        let repo = &pull_request.repository;
        if !repository.matches_names(&repo.project.name, &repo.name) {
            return Err(AzureError::WrongRepository);
        }
        Ok(pull_request)
    }

    fn get<T: DeserializeOwned>(
        &mut self,
        repository: &Repository,
        suffix: &[&str],
    ) -> Result<T, AzureError> {
        let endpoint = repository
            .git_api(suffix)
            .map_err(|_| AzureError::Endpoint)?;
        self.0.get(&endpoint)
    }
}
