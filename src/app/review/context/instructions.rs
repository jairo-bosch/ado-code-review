use ::http::StatusCode;
use serde::Serialize;

use super::{
    ReviewFile,
    azure::{AzureError, Client, Revision, Snapshot},
    exclude,
};

const ROOT_INSTRUCTIONS: &str = "/.github/copilot-instructions.md";
const INSTRUCTION_BUDGET: usize = 40_000;

#[derive(Serialize)]
pub(super) struct Instruction {
    path: String,
    applies_to: Vec<String>,
    text: String,
}

pub(super) fn gather(
    client: &mut Client,
    snapshot: &Snapshot,
    files: &[ReviewFile],
) -> Result<Vec<Instruction>, AzureError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut paths = client.instruction_paths(snapshot)?;
    paths.sort_unstable();
    paths.insert(0, ROOT_INSTRUCTIONS.into());
    let mut budget = INSTRUCTION_BUDGET;
    let mut instructions = Vec::new();
    for path in paths {
        if let Some(instruction) = read(client, snapshot, (path, files))? {
            budget = budget
                .checked_sub(instruction.text.chars().count())
                .ok_or(AzureError::Instructions)?;
            instructions.push(instruction);
        }
    }
    Ok(instructions)
}

fn read(
    client: &mut Client,
    snapshot: &Snapshot,
    (path, files): (String, &[ReviewFile]),
) -> Result<Option<Instruction>, AzureError> {
    let bytes = match client.item(snapshot, (Revision::Target, &path)) {
        Ok(bytes) => bytes,
        Err(AzureError::Status(code))
            if code == StatusCode::NOT_FOUND.as_u16() && path == ROOT_INSTRUCTIONS =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let text = String::from_utf8(bytes).map_err(|_| AzureError::Instructions)?;
    let matches = applicable(&path, &text, files)?;
    Ok((!matches.is_empty()).then_some(Instruction {
        path,
        applies_to: matches,
        text,
    }))
}

fn applicable(path: &str, text: &str, files: &[ReviewFile]) -> Result<Vec<String>, AzureError> {
    let glob = if path == ROOT_INSTRUCTIONS {
        "**"
    } else {
        let Some(glob) = frontmatter_apply_to(text) else {
            return Ok(Vec::new());
        };
        glob
    };
    let matcher =
        exclude::build(split_patterns(glob).into_iter()).map_err(|_| AzureError::Instructions)?;
    Ok(files
        .iter()
        .filter(|file| matcher.is_match(&file.path))
        .map(|file| file.path.clone())
        .collect())
}

fn frontmatter_apply_to(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim() != "---")
        .find_map(|line| line.trim_start().strip_prefix("applyTo:"))
        .map(|value| value.trim().trim_matches(['\'', '"']))
}

fn split_patterns(value: &str) -> Vec<&str> {
    let mut start = 0;
    let mut nesting = 0_usize;
    let mut patterns = Vec::new();
    for (offset, ch) in value.char_indices() {
        match ch {
            '{' | '[' => nesting += 1,
            '}' | ']' => nesting = nesting.saturating_sub(1),
            ',' if nesting == 0 => {
                patterns.extend(value.get(start..offset).map(str::trim));
                start = offset + ch.len_utf8();
            }
            _ => {}
        }
    }
    patterns.extend(value.get(start..).map(str::trim));
    patterns
}
