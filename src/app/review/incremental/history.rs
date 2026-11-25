use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    time::Duration,
};

use similar::{DiffTag, TextDiff};

use crate::app::review::{
    azure::{AzureError, Client, Revision, Snapshot},
    copilot::Review,
};

const MAX_FILE_BYTES: usize = 720_000;
const DIFF_TIMEOUT: Duration = Duration::from_secs(2);

pub(in crate::app::review) fn unchanged(
    client: &mut Client,
    (snapshot, previous): (&Snapshot, Option<NonZeroU32>),
    review: &Review,
) -> Result<BTreeSet<usize>, AzureError> {
    let mut result = BTreeSet::new();
    let Some(previous) = previous else {
        return Ok(result);
    };
    for (path, anchors) in anchors_by_path(review) {
        if previous == snapshot.iteration_id() {
            result.extend(anchors.into_iter().map(|anchor| anchor.index));
        } else {
            result.extend(unchanged_anchors(
                client,
                (snapshot, previous, &path),
                &anchors,
            )?);
        }
    }
    Ok(result)
}

struct Anchor {
    index: usize,
    start: usize,
    end: usize,
}

fn anchors_by_path(review: &Review) -> BTreeMap<String, Vec<Anchor>> {
    let mut grouped: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (index, finding) in review.new_findings.iter().enumerate() {
        if let Some(place) = &finding.location {
            grouped
                .entry(format!("/{}", place.path.trim_start_matches('/')))
                .or_default()
                .push(Anchor {
                    index,
                    start: place.start,
                    end: place.end,
                });
        }
    }
    grouped
}

fn unchanged_anchors(
    client: &mut Client,
    (snapshot, previous, path): (&Snapshot, NonZeroU32, &str),
    anchors: &[Anchor],
) -> Result<Vec<usize>, AzureError> {
    let Some(old) = client
        .historical_item(snapshot, (previous, path))?
        .and_then(source_text)
    else {
        return Ok(Vec::new());
    };
    let Some(new) = source_text(client.item(snapshot, (Revision::Head, path))?) else {
        return Ok(Vec::new());
    };
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(&old, &new);
    Ok(anchors
        .iter()
        .filter(|anchor| {
            diff.ops().iter().any(|op| {
                op.tag() == DiffTag::Equal
                    && op.new_range().start < anchor.start
                    && anchor.end <= op.new_range().end
            })
        })
        .map(|anchor| anchor.index)
        .collect())
}

fn source_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.len() > MAX_FILE_BYTES || bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}
