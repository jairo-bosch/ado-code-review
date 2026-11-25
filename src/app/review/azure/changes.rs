use std::num::NonZeroU32;

use ::http::StatusCode;
use serde::Deserialize;
use url::Url;

use super::{AzureError, Client, Revision, Snapshot, http::OCTET_STREAM, model::Collection};

const PAGE_SIZE: &str = "100";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    change_entries: Vec<Entry>,
    #[serde(default)]
    next_skip: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    change_tracking_id: Option<u32>,
    change_type: String,
    item: Item,
    original_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    path: String,
    git_object_type: Option<String>,
}

#[derive(Deserialize)]
struct InstructionItem {
    path: String,
}

pub(in crate::app::review) struct FileChange {
    pub(in crate::app::review) tracking_id: Option<NonZeroU32>,
    pub(in crate::app::review) path: String,
    pub(in crate::app::review) before: Option<String>,
    pub(in crate::app::review) after: bool,
}

impl Entry {
    fn file_change(self) -> Option<FileChange> {
        let is_type = |name| {
            self.change_type
                .split(',')
                .any(|value| value.trim().eq_ignore_ascii_case(name))
        };
        if matches!(self.item.git_object_type.as_deref(), Some("tree")) {
            return None;
        }
        let before = if is_type("add") || is_type("branch") || is_type("undelete") {
            None
        } else {
            Some(self.original_path.unwrap_or_else(|| self.item.path.clone()))
        };
        Some(FileChange {
            tracking_id: self.change_tracking_id.and_then(NonZeroU32::new),
            path: self.item.path,
            before,
            after: !is_type("delete"),
        })
    }
}

impl Client {
    pub(in crate::app::review) fn files(
        &mut self,
        snapshot: &Snapshot,
    ) -> Result<Vec<FileChange>, AzureError> {
        let mut endpoint = snapshot
            .repository
            .git_api(&[
                "pullRequests",
                &snapshot.pr.id.to_string(),
                "iterations",
                &snapshot.iteration_id().to_string(),
                "changes",
            ])
            .map_err(|_| AzureError::Endpoint)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("$top", PAGE_SIZE), ("$compareTo", "0")]);
        let mut files = Vec::new();
        let mut skip = 0;
        loop {
            let mut url = endpoint.clone();
            url.query_pairs_mut()
                .append_pair("$skip", &skip.to_string());
            let page: Page = self.0.get(&url)?;
            files.extend(
                page.change_entries
                    .into_iter()
                    .filter_map(Entry::file_change),
            );
            if page.next_skip == 0 {
                return Ok(files);
            }
            if page.next_skip <= skip {
                return Err(AzureError::InvalidChangesPage);
            }
            skip = page.next_skip;
        }
    }

    pub(in crate::app::review) fn item(
        &mut self,
        snapshot: &Snapshot,
        (revision, path): (Revision, &str),
    ) -> Result<Vec<u8>, AzureError> {
        self.content(snapshot.items_endpoint(revision)?, path)
    }

    pub(in crate::app::review) fn historical_item(
        &mut self,
        snapshot: &Snapshot,
        (iteration, path): (NonZeroU32, &str),
    ) -> Result<Option<Vec<u8>>, AzureError> {
        let Some(revision) = snapshot.pr.history.iter().find(|past| past.id == iteration) else {
            return Ok(None);
        };
        let endpoint = snapshot.commit_items_endpoint(revision.source_ref_commit.sha()?)?;
        match self.content(endpoint, path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(AzureError::Status(code)) if code == StatusCode::NOT_FOUND.as_u16() => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn content(&mut self, mut endpoint: Url, path: &str) -> Result<Vec<u8>, AzureError> {
        endpoint
            .query_pairs_mut()
            .extend_pairs([("path", path), ("download", "true")]);
        self.0.get_bytes(&endpoint, OCTET_STREAM)
    }

    pub(in crate::app::review) fn instruction_paths(
        &mut self,
        snapshot: &Snapshot,
    ) -> Result<Vec<String>, AzureError> {
        const DIRECTORY: &str = "/.github/instructions/";
        let mut endpoint = snapshot.items_endpoint(Revision::Target)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("scopePath", DIRECTORY.trim_end_matches('/')),
            ("recursionLevel", "oneLevel"),
        ]);
        let items: Collection<InstructionItem> = match self.0.get(&endpoint) {
            Ok(items) => items,
            Err(AzureError::Status(code)) if code == StatusCode::NOT_FOUND.as_u16() => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        Ok(items
            .value
            .into_iter()
            .map(|item| item.path)
            .filter(|path| {
                path.strip_prefix(DIRECTORY)
                    .is_some_and(|name| !name.contains('/') && name.ends_with(".instructions.md"))
            })
            .collect())
    }
}

impl Snapshot {
    fn items_endpoint(&self, revision: Revision) -> Result<Url, AzureError> {
        let iteration = &self.iteration;
        let commit = match revision {
            Revision::Base => &iteration.common_ref_commit,
            Revision::Head => &iteration.source_ref_commit,
            Revision::Target => &iteration.target_ref_commit,
        };
        self.commit_items_endpoint(commit.sha()?)
    }

    fn commit_items_endpoint(&self, sha: &str) -> Result<Url, AzureError> {
        let mut endpoint = self
            .repository
            .git_api(&["items"])
            .map_err(|_| AzureError::Endpoint)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("versionDescriptor.versionType", "commit"),
            ("versionDescriptor.version", sha),
        ]);
        Ok(endpoint)
    }
}
