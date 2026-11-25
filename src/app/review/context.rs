mod diff;
mod exclude;
mod instructions;
#[cfg(test)]
mod tests;

use std::{num::NonZeroU32, path::Path};

use serde::Serialize;

use super::azure::{self, AzureError, FileChange, Revision, Snapshot};

pub(super) use exclude::Exclusions;
#[cfg(test)]
pub(super) use tests::fixture;

const FULL_CONTENT_BUDGET: usize = 360_000;
const DIFF_BUDGET: usize = 360_000;
const HEADER_LINES: usize = 8;
const MAX_FILE_BYTES: usize = 720_000;

#[derive(Serialize)]
pub(super) struct Context {
    details: azure::Details,
    threads: Vec<azure::Thread>,
    files: Vec<ReviewFile>,
    skipped: Vec<SkippedFile>,
    instructions: Vec<instructions::Instruction>,
}

#[derive(Serialize)]
struct ReviewFile {
    #[serde(skip)]
    tracking_id: Option<NonZeroU32>,
    path: String,
    previous_path: Option<String>,
    hunks: Vec<diff::Hunk>,
    new_content: Option<String>,
}

#[derive(Serialize)]
struct SkippedFile {
    path: String,
    reason: SkippedReason,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SkippedReason {
    DefaultExcluded,
    UserExcluded,
    Binary,
    Generated,
    TooLarge,
}

struct Budget {
    diff: usize,
    content: usize,
}

struct FileCollector<'a> {
    exclusions: &'a Exclusions,
    budget: Budget,
}

#[derive(Default)]
struct Files {
    included: Vec<ReviewFile>,
    skipped: Vec<SkippedFile>,
}

impl Context {
    pub(super) fn gather(
        client: &mut azure::Client,
        snapshot: &Snapshot,
        exclusions: &Exclusions,
    ) -> Result<Self, AzureError> {
        let mut changes = client.files(snapshot)?;
        changes.sort_by_key(|change| priority(&change.path));
        let mut files = Files::default();
        let mut collector = FileCollector {
            exclusions,
            budget: Budget {
                diff: DIFF_BUDGET,
                content: FULL_CONTENT_BUDGET,
            },
        };
        for change in changes {
            files.record(
                &change.path,
                collector.collect(&change, |target, path| {
                    client.item(snapshot, (target, path))
                })?,
            );
        }
        let instructions = instructions::gather(client, snapshot, &files.included)?;
        Ok(Self {
            files: files.included,
            skipped: files.skipped,
            details: snapshot.details.clone(),
            threads: snapshot
                .threads
                .iter()
                .map(azure::Thread::for_model)
                .collect(),
            instructions,
        })
    }

    pub(super) const fn counts(&self) -> (usize, usize, usize) {
        (self.files.len(), self.skipped.len(), self.threads.len())
    }

    pub(super) fn has_anchor(&self, path: &str, (start, end): (usize, usize)) -> bool {
        self.files
            .iter()
            .find(|file| file.path == display_path(path))
            .is_some_and(|file| {
                file.hunks
                    .iter()
                    .any(|hunk| hunk.contains_change(start, end))
            })
    }

    pub(super) fn has_thread(&self, id: u32) -> bool {
        self.threads.iter().any(|thread| thread.id().get() == id)
    }

    pub(super) fn inline_anchor(&self, path: &str, end: usize) -> Option<(NonZeroU32, usize)> {
        let file = self
            .files
            .iter()
            .find(|file| file.path == display_path(path))?;
        let offset = file.hunks.iter().find_map(|hunk| hunk.end_offset(end))?;
        Some((file.tracking_id?, offset))
    }

    pub(super) fn anchor_fingerprint(&self, path: &str, lines: (usize, usize)) -> Option<String> {
        let file = self
            .files
            .iter()
            .find(|file| file.path == display_path(path))?;
        file.hunks.iter().find_map(|hunk| hunk.fingerprint(lines))
    }

    pub(super) fn skipped(&self) -> impl Iterator<Item = (&str, &'static str)> {
        self.skipped
            .iter()
            .map(|file| (file.path.as_str(), file.reason.label()))
    }
}

impl Files {
    fn record(&mut self, path: &str, result: Result<ReviewFile, SkippedReason>) {
        match result {
            Ok(file) => self.included.push(file),
            Err(reason) => self.skipped.push(SkippedFile {
                path: display_path(path).to_owned(),
                reason,
            }),
        }
    }
}

impl FileCollector<'_> {
    fn collect(
        &mut self,
        change: &FileChange,
        mut load: impl FnMut(Revision, &str) -> Result<Vec<u8>, AzureError>,
    ) -> Result<Result<ReviewFile, SkippedReason>, AzureError> {
        if let Some(reason) = exclusion_reason(change, self.exclusions) {
            return Ok(Err(reason));
        }
        let old = change
            .before
            .as_deref()
            .map(|path| load(Revision::Base, path))
            .transpose()?
            .unwrap_or_default();
        let new = if change.after {
            load(Revision::Head, &change.path)?
        } else {
            Vec::new()
        };
        Ok(build_file(change, (old, new), &mut self.budget))
    }
}

fn exclusion_reason(change: &FileChange, exclusions: &Exclusions) -> Option<SkippedReason> {
    exclusions.reason(&change.path).or_else(|| {
        change
            .before
            .as_deref()
            .and_then(|path| exclusions.reason(path))
    })
}

fn build_file(
    change: &FileChange,
    sides: (Vec<u8>, Vec<u8>),
    budget: &mut Budget,
) -> Result<ReviewFile, SkippedReason> {
    let old = source_text(sides.0)?;
    let new = source_text(sides.1)?;
    let hunks = diff::hunks(&old, &new);
    let size = hunks.iter().map(diff::Hunk::diff_size).sum::<usize>();
    budget.diff = budget
        .diff
        .checked_sub(size)
        .ok_or(SkippedReason::TooLarge)?;
    let new_content = change.after.then(|| budget.take_content(new)).flatten();
    let path = display_path(&change.path);
    let previous_path = change
        .before
        .as_deref()
        .map(display_path)
        .filter(|old| *old != path);
    Ok(ReviewFile {
        tracking_id: change.tracking_id,
        path: path.to_owned(),
        previous_path: previous_path.map(str::to_owned),
        hunks,
        new_content,
    })
}

impl Budget {
    fn take_content(&mut self, text: String) -> Option<String> {
        self.content = self.content.checked_sub(text.chars().count())?;
        Some(text)
    }
}

impl SkippedReason {
    const fn label(self) -> &'static str {
        match self {
            Self::DefaultExcluded => "Default exclusion",
            Self::UserExcluded => "Excluded by a user glob",
            Self::Binary => "Binary or non-UTF-8",
            Self::Generated => "Generated content",
            Self::TooLarge => "Diff exceeds context budget",
        }
    }
}

fn source_text(bytes: Vec<u8>) -> Result<String, SkippedReason> {
    if bytes.contains(&0) {
        return Err(SkippedReason::Binary);
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(SkippedReason::TooLarge);
    }
    let text = String::from_utf8(bytes).map_err(|_| SkippedReason::Binary)?;
    if text.lines().take(HEADER_LINES).any(generated_header) {
        return Err(SkippedReason::Generated);
    }
    Ok(text)
}

fn generated_header(line: &str) -> bool {
    let line = line.trim_start();
    if !["//", "#", "/*", "<!--"]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("@generated")
        || lower.contains("code generated")
        || lower.contains("do not edit")
}

fn display_path(path: &str) -> &str {
    path.trim_start_matches('/')
}

fn priority(path: &str) -> (bool, bool) {
    (
        Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md")),
        path.contains("/test") || path.contains("_test."),
    )
}
