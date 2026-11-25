use std::{io::Write, num::NonZeroU32};

use uuid::Uuid;

use super::{
    Effort, Repository,
    azure::{AzureError, Client, Inline, Snapshot},
    context::Context,
    copilot::{Finding, Mode, Review},
    incremental::{self, FINDING, PreviousReview, SUMMARY, history, resolution},
    render,
};
use crate::app::Error;

pub(super) struct Target<'a> {
    pub(super) repository: &'a Repository,
    pub(super) pr: NonZeroU32,
    pub(super) original: &'a Snapshot,
    pub(super) identity: Uuid,
    pub(super) force: bool,
}

pub(super) fn publish(
    client: &mut Client,
    target: &Target<'_>,
    (report, progress): (&mut Report<'_>, &mut impl Write),
) -> Result<(), Error> {
    let fresh = client.snapshot(target.repository, target.pr)?;
    if !target.original.matches_iteration(&fresh) {
        return stale(progress);
    }
    let previous = PreviousReview::from_threads(&fresh.threads, target.identity);
    if !target.force && previous.includes(fresh.iteration_id()) {
        writeln!(
            progress,
            "Iteration {} has already been reviewed; skipping",
            fresh.iteration_id()
        )?;
        return Ok(());
    }
    if !write_review(client, (&fresh, target.identity, previous), report)? {
        return stale(progress);
    }
    writeln!(
        progress,
        "Published review for iteration {}",
        fresh.iteration_id()
    )?;
    Ok(())
}

fn stale(progress: &mut impl Write) -> Result<(), Error> {
    writeln!(
        progress,
        "Pull request changed during review; nothing was published"
    )?;
    Ok(())
}

pub(super) struct Report<'a> {
    pub(super) context: &'a Context,
    pub(super) review: &'a mut Review,
    pub(super) effort: (Effort, Mode),
}

pub(super) fn preview(
    client: &mut Client,
    (snapshot, user, previous): (&Snapshot, Uuid, &PreviousReview),
    (report, out): (&mut Report<'_>, &mut impl Write),
) -> Result<(), Error> {
    let missed = classify_findings(client, (snapshot, previous), report.review)?;
    let publication = build_publication(None, (snapshot, user, missed), report)?;
    render::preview(
        out,
        report.context,
        (report.review, report.effort, &publication),
    )?;
    Ok(())
}

fn write_review(
    client: &mut Client,
    (snapshot, identity, previous): (&Snapshot, Uuid, PreviousReview),
    report: &mut Report<'_>,
) -> Result<bool, Error> {
    let summary = summary_target(snapshot, identity)?;
    let missed = classify_findings(client, (snapshot, &previous), report.review)?;
    if !snapshot.is_current(client)? {
        return Ok(false);
    }
    let publication = build_publication(Some(client), (snapshot, identity, missed), report)?;
    let markdown = summary_content(
        snapshot,
        report.context,
        (report.review, report.effort, &publication),
    )?;
    save_summary(client, snapshot, (summary, &markdown))?;
    Ok(true)
}

fn classify_findings(
    client: &mut Client,
    (snapshot, previous): (&Snapshot, &PreviousReview),
    review: &mut Review,
) -> Result<std::collections::BTreeSet<usize>, AzureError> {
    previous.filter_findings(review);
    history::unchanged(client, (snapshot, previous.latest()), review)
}

fn save_summary(
    client: &mut Client,
    snapshot: &Snapshot,
    (target, markdown): (Option<(NonZeroU32, NonZeroU32)>, &str),
) -> Result<(), AzureError> {
    match target {
        Some((thread, comment)) => client.update_summary(snapshot, (thread, comment, markdown))?,
        None => {
            client.create_thread(snapshot, (markdown, None))?;
        }
    }
    Ok(())
}

fn build_publication(
    mut client: Option<&mut Client>,
    (snapshot, user, missed): (&Snapshot, Uuid, std::collections::BTreeSet<usize>),
    report: &mut Report<'_>,
) -> Result<render::Publication, Error> {
    let mut publication = render::Publication {
        missed,
        ..render::Publication::default()
    };
    publication.links = record_findings(
        client.as_deref_mut(),
        (snapshot, user),
        (report.context, report.review),
    )?;
    record_existing(client, (snapshot, user), (report, &mut publication))?;
    Ok(publication)
}

fn record_existing(
    mut client: Option<&mut Client>,
    (snapshot, user): (&Snapshot, Uuid),
    (report, publication): (&Report<'_>, &mut render::Publication),
) -> Result<(), AzureError> {
    for existing in snapshot
        .threads
        .iter()
        .filter(|thread| thread.is_active())
        .filter_map(|thread| resolution::Existing::from_thread(thread, user))
    {
        let url = snapshot.discussion_url(existing.thread.id());
        if publication.links.iter().any(|(_, link)| *link == url) {
            continue;
        }
        let row = render::discussion(existing.heading, &url, existing.thread.anchor());
        if let Some(action) = existing.resolution(user, (report.context, report.review)) {
            if let Some(client) = client.as_deref_mut() {
                client.resolve_thread(snapshot, (existing.thread.id(), action.0, action.1))?;
            }
            publication.resolved.push(row);
        } else {
            publication.open.push(row);
        }
    }
    Ok(())
}

fn summary_target(
    snapshot: &Snapshot,
    user: Uuid,
) -> Result<Option<(NonZeroU32, NonZeroU32)>, AzureError> {
    let mut summaries = snapshot.threads.iter().filter(|thread| {
        thread
            .owned_comment(user)
            .and_then(|comment| comment.lines().next())
            == Some(SUMMARY)
    });
    let Some(thread) = summaries.next() else {
        return Ok(None);
    };
    if summaries.next().is_some() {
        return Err(AzureError::ProtectedSummary);
    }
    let comment = thread
        .editable_summary(user)
        .ok_or(AzureError::ProtectedSummary)?;
    Ok(Some((thread.id(), comment)))
}

fn record_findings(
    mut client: Option<&mut Client>,
    (snapshot, user): (&Snapshot, Uuid),
    (context, review): (&Context, &mut Review),
) -> Result<Vec<(usize, url::Url)>, Error> {
    let mut links = Vec::new();
    for (index, finding) in review.new_findings.iter_mut().enumerate() {
        if let Some(id) =
            record_finding(client.as_deref_mut(), (snapshot, user), (context, finding))?
        {
            links.push((index, snapshot.discussion_url(id)));
        }
    }
    Ok(links)
}

fn record_finding(
    client: Option<&mut Client>,
    (snapshot, user): (&Snapshot, Uuid),
    (context, finding): (&Context, &mut Finding),
) -> Result<Option<NonZeroU32>, Error> {
    let Some(anchor) = inline_anchor(context, finding) else {
        finding.location = None;
        return Ok(None);
    };
    match (existing_finding(snapshot, user, (&anchor, finding)), client) {
        (None, Some(client)) => {
            post_finding(client, (snapshot, context), (anchor, finding)).map(Some)
        }
        (id, _) => Ok(id),
    }
}

fn inline_anchor<'a>(context: &Context, finding: &'a Finding) -> Option<Inline<'a>> {
    let location = finding.location.as_ref()?;
    let (tracking_id, end_offset) = context.inline_anchor(&location.path, location.end)?;
    Some(Inline {
        path: &location.path,
        lines: (location.start, location.end),
        end_offset,
        tracking_id,
    })
}

fn existing_finding(
    snapshot: &Snapshot,
    user: Uuid,
    (anchor, finding): (&Inline<'_>, &Finding),
) -> Option<NonZeroU32> {
    let heading = render::inline_heading(finding);
    snapshot.threads.iter().find_map(|thread| {
        let (path, start, end) = thread.anchor()?;
        if (path.trim_start_matches('/'), start, end)
            != (
                anchor.path.trim_start_matches('/'),
                anchor.lines.0,
                anchor.lines.1,
            )
        {
            return None;
        }
        let mut lines = thread.owned_comment(user)?.lines();
        (thread.is_active() && lines.next() == Some(FINDING) && lines.next() == Some(&heading))
            .then_some(thread.id())
    })
}

fn post_finding(
    client: &mut Client,
    (snapshot, context): (&Snapshot, &Context),
    (anchor, finding): (Inline<'_>, &Finding),
) -> Result<NonZeroU32, Error> {
    let mut body = Vec::new();
    writeln!(body, "{FINDING}")?;
    let fingerprint = context
        .anchor_fingerprint(anchor.path, anchor.lines)
        .ok_or(AzureError::Response)?;
    render::inline(&mut body, finding, Some(&resolution::marker(&fingerprint)))?;
    Ok(client.create_thread(snapshot, (&decode(body)?, Some(anchor)))?)
}

fn summary_content(
    snapshot: &Snapshot,
    context: &Context,
    (review, effort, publication): (&Review, (Effort, Mode), &render::Publication),
) -> Result<String, Error> {
    let mut body = Vec::new();
    writeln!(
        body,
        "{SUMMARY}\n{}",
        incremental::run_marker(snapshot.iteration_id())
    )?;
    render::summary(&mut body, context, (review, effort, publication))?;
    decode(body)
}

fn decode(bytes: Vec<u8>) -> Result<String, Error> {
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error).into())
}
