use std::{fs, path::Path};

use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Quality {
    pub(super) size: Size,
    pub(super) function: Function,
    pub(super) counts: Counts,
    pub(super) tools: Tools,
    #[serde(default)]
    pub(super) exceptions: Vec<Exception>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Size {
    pub(super) file_lines: usize,
    pub(super) impl_lines: usize,
    pub(super) fields: usize,
    pub(super) public_methods: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Function {
    pub(super) cyclomatic: usize,
    pub(super) cognitive: usize,
    pub(super) branches: usize,
    pub(super) boolean_operands: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    pub(super) statements: usize,
    pub(super) locals: usize,
    pub(super) exits: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Tools {
    pub(super) coverage: u8,
    pub(super) duplicate_lines: usize,
    pub(super) duplicate_threshold: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Exception {
    pub(super) path: String,
    pub(super) rule: String,
    reason: String,
}

#[derive(Deserialize)]
struct Manifest {
    workspace: Workspace,
}

#[derive(Deserialize)]
struct Workspace {
    metadata: Metadata,
}

#[derive(Deserialize)]
struct Metadata {
    quality: Quality,
}

impl Quality {
    pub(super) fn load(manifest: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(manifest).context("cannot read workspace manifest")?;
        Self::parse(&content)
    }

    pub(super) fn parse(content: &str) -> anyhow::Result<Self> {
        let parsed: Manifest = toml::from_str(content).context("invalid quality configuration")?;
        let quality = parsed.workspace.metadata.quality;
        for exception in &quality.exceptions {
            anyhow::ensure!(
                !exception.reason.trim().is_empty(),
                "exception reason must not be blank"
            );
            anyhow::ensure!(
                super::findings::RULES.contains(&exception.rule.as_str()),
                "unknown rule: {}",
                exception.rule
            );
            anyhow::ensure!(
                !exception.path.is_empty()
                    && Path::new(&exception.path)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "expected a repository-relative exception path: {}",
                exception.path,
            );
        }
        Ok(quality)
    }
}
