use std::io::{self, Write};

#[cfg(test)]
mod tests;

use super::{
    Effort,
    context::Context,
    copilot::{Finding, Mode, Review, Severity},
};

#[derive(Default)]
pub(super) struct Publication {
    pub(super) links: Vec<(usize, url::Url)>,
    pub(super) open: Vec<Discussion>,
    pub(super) resolved: Vec<Discussion>,
    pub(super) missed: std::collections::BTreeSet<usize>,
}

pub(super) struct Discussion {
    pub(super) severity: Severity,
    pub(super) label: String,
}

impl Severity {
    const fn badge(self) -> &'static str {
        match self {
            Self::Low => "🟡 **Low**",
            Self::Medium => "🟠 **Medium**",
            Self::High => "🔴 **High**",
        }
    }

    const fn verdict(self) -> &'static str {
        match self {
            Self::Low => "🟡 Needs a closer look",
            Self::Medium => "🟠 Changes recommended",
            Self::High => "🔴 Changes required",
        }
    }
}

pub(super) fn preview(
    out: &mut impl Write,
    context: &Context,
    (review, effort, publication): (&Review, (Effort, Mode), &Publication),
) -> io::Result<()> {
    summary(out, context, (review, effort, publication))?;
    for (_, finding) in anchored(review) {
        writeln!(out, "\n---")?;
        inline(out, finding, None)?;
    }
    Ok(())
}

pub(super) fn summary(
    out: &mut impl Write,
    context: &Context,
    (review, effort, publication): (&Review, (Effort, Mode), &Publication),
) -> io::Result<()> {
    let verdict = verdict(review, publication);
    writeln!(out, "### {verdict}\n\n{}", text(&review.overview))?;
    write_open(out, review, publication)?;
    write_existing(out, publication)?;
    write_other(out, &review.new_findings)?;
    write_skipped(out, context)?;
    writeln!(
        out,
        "\n---\n_Review effort: {} · {}_",
        effort_label(effort),
        effort.1.model()
    )
}

fn verdict(review: &Review, publication: &Publication) -> &'static str {
    let existing = publication.open.iter().map(|row| row.severity);
    review
        .new_findings
        .iter()
        .map(|finding| finding.severity)
        .chain(existing)
        .max()
        .map_or("✅ No issues found", Severity::verdict)
}

fn write_open(out: &mut impl Write, review: &Review, publication: &Publication) -> io::Result<()> {
    let list = anchored(review);
    let open = list.len() + publication.open.len();
    if open != 0 {
        writeln!(out, "\n**{open} open issues**")?;
    }
    for (index, finding) in list {
        let link = publication.links.iter().find(|(key, _)| *key == index);
        let missed = publication.missed.contains(&index);
        let state = if missed { "Previously missed" } else { "New" };
        writeln!(
            out,
            "- {} · {state}",
            label(finding, link.map(|(_, url)| url))
        )?;
    }
    Ok(())
}

fn write_existing(out: &mut impl Write, result: &Publication) -> io::Result<()> {
    for row in &result.open {
        writeln!(out, "- {} · Still open", row.label)?;
    }
    if !result.resolved.is_empty() {
        writeln!(out, "\n**Resolved since last review**")?;
        for row in &result.resolved {
            writeln!(out, "- {}", row.label)?;
        }
    }
    Ok(())
}

pub(super) fn discussion(
    heading: &str,
    url: &url::Url,
    anchor: Option<(&str, usize, usize)>,
) -> Discussion {
    let (severity, title) = [Severity::High, Severity::Medium, Severity::Low]
        .into_iter()
        .find_map(|severity| {
            Some((
                severity,
                heading
                    .strip_prefix(severity.badge())?
                    .strip_prefix(" · ")?,
            ))
        })
        .unwrap_or((Severity::Low, heading));
    let place = anchor.map_or_else(String::new, |(path, start, end)| {
        format!(" · {}", position(path, start, end))
    });
    let title = link_title(title);
    Discussion {
        severity,
        label: format!("{} · [{title}](<{url}>){place}", severity.badge()),
    }
}

fn anchored(review: &Review) -> Vec<(usize, &Finding)> {
    let mut list: Vec<_> = review
        .new_findings
        .iter()
        .enumerate()
        .filter(|(_, finding)| finding.location.is_some())
        .collect();
    list.sort_by_key(|(_, finding)| std::cmp::Reverse(finding.severity));
    list
}

fn write_other(out: &mut impl Write, findings: &[Finding]) -> io::Result<()> {
    let list: Vec<_> = findings
        .iter()
        .filter(|finding| finding.location.is_none())
        .collect();
    if !list.is_empty() {
        writeln!(out, "\n**Other findings**")?;
    }
    for finding in list {
        writeln!(out, "\n{}\n\n{}", label(finding, None), text(&finding.body))?;
    }
    Ok(())
}

fn write_skipped(out: &mut impl Write, context: &Context) -> io::Result<()> {
    let mut files = context.skipped().peekable();
    if files.peek().is_some() {
        writeln!(
            out,
            "\n**Skipped files**\n\n| File | Reason |\n| --- | --- |"
        )?;
    }
    for (path, reason) in files {
        writeln!(
            out,
            "| {} | {reason} |",
            code_span(path).replace('|', "\\|")
        )?;
    }
    Ok(())
}

pub(super) fn inline(
    out: &mut impl Write,
    finding: &Finding,
    marker: Option<&str>,
) -> io::Result<()> {
    if let Some(marker) = marker {
        writeln!(out, "{}\n{marker}", inline_heading(finding))?;
    } else {
        writeln!(out, "{}", label(finding, None))?;
    }
    writeln!(out, "\n{}", text(&finding.body))?;
    if let Some(suggestion) = finding
        .suggestion
        .as_deref()
        .filter(|text| !text.contains("```"))
    {
        let replacement = strip_controls(suggestion);
        writeln!(
            out,
            "\n```suggestion\n{}\n```",
            replacement.trim_end_matches('\n')
        )?;
    }
    Ok(())
}

pub(super) fn inline_heading(finding: &Finding) -> String {
    format!("{} · {}", finding.severity.badge(), title(finding))
}

fn title(finding: &Finding) -> String {
    text(&finding.title).replace('\n', " ")
}

fn link_title(value: &str) -> String {
    text(value)
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn label(finding: &Finding, link: Option<&url::Url>) -> String {
    let mut label = link.map_or_else(
        || inline_heading(finding),
        |url| {
            let title = link_title(&title(finding));
            format!("{} · [{title}](<{url}>)", finding.severity.badge())
        },
    );
    if let Some(location) = &finding.location {
        label = format!(
            "{label} · {}",
            position(&location.path, location.start, location.end)
        );
    }
    label
}

fn position(path: &str, start: usize, end: usize) -> String {
    let path = path.trim_start_matches('/');
    let value = if start == end {
        format!("{path}:{start}")
    } else {
        format!("{path}:{start}-{end}")
    };
    code_span(&value)
}

const fn effort_label((effort, mode): (Effort, Mode)) -> &'static str {
    match (effort, mode) {
        (Effort::Adaptive, Mode::Lite) => "Adaptive → Lite",
        (Effort::Adaptive, Mode::Balanced) => "Adaptive → Balanced",
        (_, Mode::Lite) => "Lite",
        (_, Mode::Balanced) => "Balanced",
    }
}

fn code_span(value: &str) -> String {
    let clean = text(value).replace('`', "&#96;").replace('\n', " ");
    format!("`{clean}`")
}

fn text(value: &str) -> String {
    strip_controls(value)
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn strip_controls(value: &str) -> String {
    value
        .chars()
        .filter(|&ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}
