use std::num::NonZeroU32;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{AzureError, Client, Snapshot};

pub(in crate::app::review) struct Inline<'a> {
    pub(in crate::app::review) path: &'a str,
    pub(in crate::app::review) lines: (usize, usize),
    pub(in crate::app::review) end_offset: usize,
    pub(in crate::app::review) tracking_id: NonZeroU32,
}

#[derive(Deserialize)]
struct Created {
    id: NonZeroU32,
}

impl Client {
    pub(in crate::app::review) fn create_thread(
        &mut self,
        snapshot: &Snapshot,
        (content, inline): (&str, Option<Inline<'_>>),
    ) -> Result<NonZeroU32, AzureError> {
        let endpoint = snapshot.thread_endpoint(&[])?;
        let body = thread_body(content, inline, snapshot.iteration_id());
        let created: Created = self.0.write(&endpoint, ("POST", &body))?;
        Ok(created.id)
    }

    pub(in crate::app::review) fn update_summary(
        &mut self,
        snapshot: &Snapshot,
        (thread, comment, content): (NonZeroU32, NonZeroU32, &str),
    ) -> Result<(), AzureError> {
        let endpoint =
            snapshot.thread_endpoint(&[&thread.to_string(), "comments", &comment.to_string()])?;
        let updated: Created = self
            .0
            .write(&endpoint, ("PATCH", &json!({"content": content})))?;
        if updated.id != comment {
            return Err(AzureError::Response);
        }
        Ok(())
    }

    pub(in crate::app::review) fn resolve_thread(
        &mut self,
        snapshot: &Snapshot,
        (thread, root, reply): (NonZeroU32, NonZeroU32, Option<&str>),
    ) -> Result<(), AzureError> {
        if let Some(content) = reply {
            let endpoint = snapshot.thread_endpoint(&[&thread.to_string(), "comments"])?;
            let body = json!({"parentCommentId":root, "commentType":"text", "content":content});
            let _: Created = self.0.write(&endpoint, ("POST", &body))?;
        }
        let endpoint = snapshot.thread_endpoint(&[&thread.to_string()])?;
        let updated: Created = self
            .0
            .write(&endpoint, ("PATCH", &json!({"status":"fixed"})))?;
        if updated.id != thread {
            return Err(AzureError::Response);
        }
        Ok(())
    }
}

impl Snapshot {
    fn thread_endpoint(&self, suffix: &[&str]) -> Result<url::Url, AzureError> {
        let id = self.pr.id.to_string();
        let segments: Vec<_> = ["pullRequests", id.as_str(), "threads"]
            .into_iter()
            .chain(suffix.iter().copied())
            .collect();
        self.repository
            .git_api(&segments)
            .map_err(|_| AzureError::Endpoint)
    }
}

fn thread_body(content: &str, inline: Option<Inline<'_>>, iteration: NonZeroU32) -> Value {
    let mut body = serde_json::Map::from_iter([
        (
            "comments".into(),
            json!([{"parentCommentId": 0, "commentType": "text", "content": content}]),
        ),
        (
            "status".into(),
            json!(if inline.is_some() { "active" } else { "closed" }),
        ),
    ]);
    if let Some(inline) = inline {
        body.insert(
            "threadContext".into(),
            json!({
                "filePath": format!("/{}", inline.path.trim_start_matches('/')),
                "rightFileStart": {"line": inline.lines.0, "offset": 1},
                "rightFileEnd": {"line": inline.lines.1, "offset": inline.end_offset}
            }),
        );
        body.insert("pullRequestThreadContext".into(), json!({
            "changeTrackingId": inline.tracking_id,
            "iterationContext": {"firstComparingIteration": 0, "secondComparingIteration": iteration}
        }));
    }
    Value::Object(body)
}
