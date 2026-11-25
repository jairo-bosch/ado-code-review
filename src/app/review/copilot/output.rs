use serde::Deserialize;
use serde_json::{Value, json};

use super::{super::context::Context, CopilotError};

const TITLE_LIMIT: usize = 80;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::review) struct Review {
    pub(in crate::app::review) overview: String,
    pub(in crate::app::review) new_findings: Vec<Finding>,
    existing: Vec<Existing>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::review) struct Finding {
    pub(in crate::app::review) title: String,
    pub(in crate::app::review) body: String,
    pub(in crate::app::review) severity: Severity,
    pub(in crate::app::review) location: Option<Location>,
    pub(in crate::app::review) suggestion: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(in crate::app::review) enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::review) struct Location {
    pub(in crate::app::review) path: String,
    pub(in crate::app::review) start: usize,
    pub(in crate::app::review) end: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Existing {
    id: u32,
    status: Status,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    StillOpen,
    Resolved,
}

impl Review {
    pub(in crate::app::review) fn wants_resolution(&self, id: u32) -> bool {
        self.existing
            .iter()
            .any(|existing| existing.id == id && matches!(existing.status, Status::Resolved))
    }

    pub(super) fn validate(&self, context: &Context) -> Result<(), CopilotError> {
        if self.overview.trim().is_empty() {
            return Err(CopilotError::Invalid("overview must not be empty"));
        }
        for finding in &self.new_findings {
            finding.validate(context)?;
        }
        validate_thread_ids(&self.existing, context)
    }
}

impl Finding {
    fn validate(&self, context: &Context) -> Result<(), CopilotError> {
        if self.title.trim().is_empty() || self.title.chars().count() >= TITLE_LIMIT {
            return Err(CopilotError::Invalid("titles must have 1–79 characters"));
        }
        if self.body.trim().is_empty() {
            return Err(CopilotError::Invalid("finding bodies must not be empty"));
        }
        if let Some(place) = &self.location
            && !context.has_anchor(&place.path, (place.start, place.end))
        {
            return Err(CopilotError::Invalid(
                "locations must cover changed new-side lines",
            ));
        }
        Ok(())
    }
}

fn validate_thread_ids(review: &[Existing], context: &Context) -> Result<(), CopilotError> {
    let mut ids = std::collections::BTreeSet::new();
    for existing in review {
        match existing.status {
            Status::StillOpen | Status::Resolved => {
                if !context.has_thread(existing.id) || !ids.insert(existing.id) {
                    return Err(CopilotError::Invalid(
                        "existing discussion IDs must be known and unique",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn schema() -> Value {
    let finding = json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "title": {"type": "string", "minLength": 1, "maxLength": TITLE_LIMIT - 1},
            "body": {"type": "string"},
            "severity": {"enum": ["high", "medium", "low"]},
            "location": {
                "type": ["object", "null"], "additionalProperties": false,
                "properties": {
                    "path": {"type": "string"},
                    "start": {"type": "integer", "minimum": 1},
                    "end": {"type": "integer", "minimum": 1}
                },
                "required": ["path", "start", "end"]
            },
            "suggestion": {"type": ["string", "null"]}
        },
        "required": ["title", "body", "severity", "location", "suggestion"]
    });
    json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "overview": {"type": "string"},
            "new_findings": {"type": "array", "items": finding},
            "existing": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "properties": {
                    "id": {"type": "integer", "minimum": 1},
                    "status": {"enum": ["still_open", "resolved"]}
                },
                "required": ["id", "status"]
            }}
        },
        "required": ["overview", "new_findings", "existing"]
    })
}
