use std::{fmt::Write as _, time::Duration};

use serde::Serialize;
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};

const CONTEXT_LINES: usize = 3;
const DIFF_TIMEOUT: Duration = Duration::from_secs(2);
const LINE_METADATA_BYTES: usize = 80;

#[derive(Serialize)]
pub(super) struct Hunk {
    lines: Vec<Line>,
}

#[derive(Serialize)]
struct Line {
    old: Option<usize>,
    new: Option<usize>,
    kind: Kind,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Context,
    Removed,
    Added,
}

impl Hunk {
    pub(super) fn end_offset(&self, end: usize) -> Option<usize> {
        let line = self.lines.iter().find(|line| line.new == Some(end))?;
        Some(
            line.text
                .trim_end_matches(['\r', '\n'])
                .encode_utf16()
                .count()
                + 1,
        )
    }

    pub(super) fn fingerprint(&self, (start, end): (usize, usize)) -> Option<String> {
        let line_count = end.checked_sub(start)?.checked_add(1)?;
        let lines = self.lines.iter().filter(|line| {
            line.new
                .is_some_and(|number| (start..=end).contains(&number))
        });
        if lines.clone().count() != line_count {
            return None;
        }
        let hash = lines.fold(Sha256::new(), |mut hash, line| {
            hash.update(line.text.as_bytes());
            hash
        });
        hash.finalize()
            .iter()
            .try_fold(String::new(), |mut output, byte| {
                write!(output, "{byte:02x}").ok()?;
                Some(output)
            })
    }

    pub(super) fn diff_size(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.text.len() + LINE_METADATA_BYTES)
            .sum()
    }

    pub(super) fn contains_change(&self, start: usize, end: usize) -> bool {
        if start == 0 || end < start {
            return false;
        }
        let len = end - start + 1;
        let covered = self.lines.iter().filter(|line| {
            line.new
                .is_some_and(|number| (start..=end).contains(&number))
        });
        covered.clone().count() == len
            && covered
                .into_iter()
                .any(|line| matches!(line.kind, Kind::Added))
    }
}

pub(super) fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);
    diff.grouped_ops(CONTEXT_LINES)
        .into_iter()
        .map(|ops| Hunk {
            lines: ops
                .iter()
                .flat_map(|op| diff.iter_changes(op))
                .map(|change| Line {
                    old: change.old_index().map(|line| line + 1),
                    new: change.new_index().map(|line| line + 1),
                    kind: match change.tag() {
                        ChangeTag::Equal => Kind::Context,
                        ChangeTag::Delete => Kind::Removed,
                        ChangeTag::Insert => Kind::Added,
                    },
                    text: change.value().to_owned(),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_new_side_lines_and_keeps_three_lines_of_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        let hunks = hunks(old, new);
        let [hunk] = hunks.as_slice() else {
            panic!("expected one hunk");
        };
        assert_eq!(
            hunk.lines
                .iter()
                .filter_map(|line| line.new)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5, 6, 7, 8]
        );
        assert!(hunk.contains_change(4, 6));
        assert!(!hunk.contains_change(2, 4));
        assert!(!hunk.contains_change(1, 5));
    }
}
