use std::{fmt, path::PathBuf};

use proc_macro2::Span;

use super::config::Quality;

pub(super) const RULES: &[&str] = &[
    "file-lines",
    "impl-lines",
    "fields",
    "public-methods",
    "cyclomatic",
    "cognitive",
    "branches",
    "boolean-operands",
    "statements",
    "locals",
    "exits",
    "magic-number",
];

#[derive(Debug)]
pub(super) struct Finding {
    pub(super) path: PathBuf,
    pub(super) rule: &'static str,
    line: usize,
    detail: String,
}

impl Finding {
    pub(super) fn applies(&self, quality: &Quality) -> bool {
        !quality
            .exceptions
            .iter()
            .any(|entry| self.path == std::path::Path::new(&entry.path) && self.rule == entry.rule)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "{}:{}: {}: {}",
            self.path.display(),
            self.line,
            self.rule,
            self.detail
        )
    }
}

pub(super) struct Report {
    pub(super) path: PathBuf,
    pub(super) findings: Vec<Finding>,
}

impl Report {
    pub(super) fn limit(&mut self, site: (Span, &'static str), counts: (usize, usize)) {
        let (actual, maximum) = counts;
        if actual > maximum {
            self.add(site, format!("{actual} exceeds limit {maximum}"));
        }
    }

    pub(super) fn add(&mut self, (span, rule): (Span, &'static str), detail: String) {
        self.findings.push(Finding {
            path: self.path.clone(),
            line: span.start().line,
            rule,
            detail,
        });
    }
}
