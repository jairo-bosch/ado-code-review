use std::num::NonZeroU32;

use uuid::Uuid;

use super::{azure::Thread, copilot::Review};

pub(super) mod history;
pub(super) mod resolution;

pub(super) const SUMMARY: &str = "<!-- ado-code-review:summary:v1 -->";
pub(super) const FINDING: &str = "<!-- ado-code-review:finding:v1 -->";
const RUN_PREFIX: &str = "<!-- ado-code-review:iteration:";
const MARKER_END: &str = " -->";

pub(super) struct PreviousReview {
    latest: Option<NonZeroU32>,
    handled: Vec<(String, usize, usize)>,
}

impl PreviousReview {
    pub(super) fn from_threads(threads: &[Thread], user: Uuid) -> Self {
        let mut state = Self {
            latest: None,
            handled: Vec::new(),
        };
        for thread in threads {
            let Some(comment) = thread.owned_comment(user) else {
                continue;
            };
            let mut lines = comment.lines();
            match lines.next() {
                Some(SUMMARY) => state.latest = state.latest.max(lines.next().and_then(iteration)),
                Some(FINDING) if thread.human_handled(user) => state.record_handled(thread),
                _ => {}
            }
        }
        state
    }

    fn record_handled(&mut self, thread: &Thread) {
        if let Some((path, start, end)) = thread.anchor() {
            self.handled
                .push((path.trim_start_matches('/').to_owned(), start, end));
        }
    }

    pub(super) fn includes(&self, iteration: NonZeroU32) -> bool {
        self.latest.is_some_and(|last| last >= iteration)
    }

    pub(super) const fn latest(&self) -> Option<NonZeroU32> {
        self.latest
    }

    pub(super) fn filter_findings(&self, review: &mut Review) {
        review.new_findings.retain(|finding| {
            !finding.location.as_ref().is_some_and(|location| {
                self.handled.iter().any(|(path, start, end)| {
                    path == location.path.trim_start_matches('/')
                        && *start <= location.end
                        && location.start <= *end
                })
            })
        });
    }
}

fn iteration(line: &str) -> Option<NonZeroU32> {
    line.strip_prefix(RUN_PREFIX)?
        .strip_suffix(MARKER_END)?
        .parse()
        .ok()
}

pub(super) fn run_marker(iteration: NonZeroU32) -> String {
    format!("{RUN_PREFIX}{iteration}{MARKER_END}")
}

#[cfg(test)]
mod tests;
