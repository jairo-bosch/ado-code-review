use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct Collection<T> {
    pub(super) value: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConnectionData {
    pub(super) authenticated_user: Identity,
}

#[derive(Clone, Deserialize)]
pub(super) struct Identity {
    pub(super) id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub(super) id: NonZeroU32,
    pub(super) repository: RemoteRepository,
    #[serde(flatten)]
    pub(super) details: Details,
}

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::app::review) struct Details {
    title: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
pub(super) struct RemoteRepository {
    pub(super) id: Uuid,
    pub(super) name: String,
    pub(super) project: Project,
}

#[derive(Deserialize)]
pub(super) struct Project {
    pub(super) id: Uuid,
    pub(super) name: String,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Iteration {
    pub(super) id: NonZeroU32,
    pub(super) common_ref_commit: Commit,
    pub(super) source_ref_commit: Commit,
    pub(super) target_ref_commit: Commit,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Commit {
    commit_id: String,
}

impl Commit {
    pub(super) fn sha(&self) -> Result<&str, super::AzureError> {
        const SHA1_LENGTH: usize = 40;
        if self.commit_id.len() != SHA1_LENGTH
            || !self.commit_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(super::AzureError::Response);
        }
        Ok(&self.commit_id)
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub(in crate::app::review) struct Thread {
    id: NonZeroU32,
    status: String,
    #[serde(default, skip_serializing)]
    is_deleted: bool,
    #[serde(rename(deserialize = "threadContext"))]
    context: Option<ThreadContext>,
    comments: Vec<Comment>,
}

impl Thread {
    pub(in crate::app::review) const fn id(&self) -> NonZeroU32 {
        self.id
    }

    pub(super) fn keep_live(&mut self) -> bool {
        self.comments.retain(|comment| !comment.is_deleted);
        !self.is_deleted
    }

    pub(in crate::app::review) fn for_model(&self) -> Self {
        let mut thread = self.clone();
        for comment in &mut thread.comments {
            if let Some(text) = &mut comment.content {
                *text = text
                    .split_inclusive('\n')
                    .filter(|line| !line.starts_with("<!-- ado-code-review:"))
                    .collect();
            }
        }
        thread
    }

    pub(in crate::app::review) fn owned_comment(&self, user: Uuid) -> Option<&str> {
        self.root_by(user)?.content.as_deref()
    }

    pub(in crate::app::review) fn editable_summary(&self, user: Uuid) -> Option<NonZeroU32> {
        if self.status != "closed" || self.has_other_reply(user) {
            return None;
        }
        self.root_by(user)?.id
    }

    pub(in crate::app::review) fn active_finding_root(&self, user: Uuid) -> Option<NonZeroU32> {
        self.is_active().then_some(())?;
        if self.has_other_reply(user) {
            return None;
        }
        self.root_by(user)?.id
    }

    pub(in crate::app::review) fn is_active(&self) -> bool {
        self.status.eq_ignore_ascii_case("active")
    }

    pub(in crate::app::review) fn has_reply(&self, user: Uuid, content: &str) -> bool {
        self.comments.iter().any(|comment| {
            comment.parent_comment_id != 0
                && comment.is_by(user)
                && comment.content.as_deref() == Some(content)
        })
    }

    fn root_by(&self, user: Uuid) -> Option<&Comment> {
        self.comments
            .iter()
            .find(|root| root.parent_comment_id == 0)
            .filter(|root| root.is_by(user))
    }

    fn has_other_reply(&self, user: Uuid) -> bool {
        self.comments.iter().any(|comment| !comment.is_by(user))
    }

    pub(in crate::app::review) fn human_handled(&self, user: Uuid) -> bool {
        ["closed", "wontfix", "bydesign"]
            .iter()
            .any(|status| self.status.eq_ignore_ascii_case(status))
            || self.has_other_reply(user)
    }

    pub(in crate::app::review) fn anchor(&self) -> Option<(&str, usize, usize)> {
        let context = self.context.as_ref()?;
        let path = context.file_path.as_deref()?;
        let start = context.right_file_start.as_ref()?.line;
        let end = context.right_file_end.as_ref()?.line;
        (start > 0 && start <= end).then_some((path, start, end))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "camelCase"))]
struct ThreadContext {
    file_path: Option<String>,
    right_file_start: Option<Line>,
    right_file_end: Option<Line>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Line {
    line: usize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "camelCase"))]
struct Comment {
    #[serde(skip_serializing)]
    id: Option<NonZeroU32>,
    content: Option<String>,
    #[serde(skip_serializing)]
    author: Option<Identity>,
    #[serde(default, skip_serializing)]
    parent_comment_id: u32,
    #[serde(default, skip_serializing)]
    is_deleted: bool,
}

impl Comment {
    fn is_by(&self, user: Uuid) -> bool {
        self.author.as_ref().is_some_and(|author| author.id == user)
    }
}
