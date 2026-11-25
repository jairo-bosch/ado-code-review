use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use super::SkippedReason;

const DEFAULT_PATTERNS: &[&str] = &[
    "**/*.lock",
    "**/*.lockb",
    "**/package-lock.json",
    "**/npm-shrinkwrap.json",
    "**/pnpm-lock.yaml",
    "**/Pipfile.lock",
    "**/*.min.*",
    "**/*.generated.*",
    "**/*.g.dart",
    "**/*.pb.go",
    "**/*.pb.cc",
    "**/*.pb.h",
];

pub(in crate::app::review) struct Exclusions {
    defaults: GlobSet,
    user: GlobSet,
}

impl Exclusions {
    pub(in crate::app::review) fn new(patterns: &[String]) -> Result<Self, globset::Error> {
        Ok(Self {
            defaults: build(DEFAULT_PATTERNS.iter().copied())?,
            user: build(patterns.iter().map(String::as_str))?,
        })
    }

    pub(super) fn reason(&self, path: &str) -> Option<SkippedReason> {
        let path = path.trim_start_matches('/');
        if self.user.is_match(path) {
            return Some(SkippedReason::UserExcluded);
        }
        self.defaults
            .is_match(path)
            .then_some(SkippedReason::DefaultExcluded)
    }
}

pub(super) fn build<'a>(
    patterns: impl Iterator<Item = &'a str>,
) -> Result<GlobSet, globset::Error> {
    let mut set = GlobSetBuilder::new();
    for pattern in patterns {
        set.add(
            GlobBuilder::new(pattern.trim_start_matches('/'))
                .literal_separator(true)
                .build()?,
        );
    }
    set.build()
}
