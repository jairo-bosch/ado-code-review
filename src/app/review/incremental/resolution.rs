use std::num::NonZeroU32;

use uuid::Uuid;

use super::{FINDING, MARKER_END};
use crate::app::review::{azure::Thread, context::Context, copilot::Review};

const PREFIX: &str = "<!-- ado-code-review:anchor:";
pub(in crate::app::review) const REPLY: &str =
    "The referenced lines changed, and the latest review no longer finds this issue.";

pub(in crate::app::review) fn marker(fingerprint: &str) -> String {
    format!("{PREFIX}{fingerprint}{MARKER_END}")
}

pub(in crate::app::review) struct Existing<'a> {
    pub(in crate::app::review) thread: &'a Thread,
    pub(in crate::app::review) heading: &'a str,
    fingerprint: Option<&'a str>,
}

impl<'a> Existing<'a> {
    pub(in crate::app::review) fn from_thread(thread: &'a Thread, user: Uuid) -> Option<Self> {
        let mut lines = thread.owned_comment(user)?.lines();
        if lines.next()? != FINDING {
            return None;
        }
        let heading = lines.next()?;
        let fingerprint = lines.next().and_then(parse_marker);
        Some(Self {
            thread,
            heading,
            fingerprint,
        })
    }

    pub(in crate::app::review) fn resolution(
        &self,
        user: Uuid,
        (context, review): (&Context, &Review),
    ) -> Option<(NonZeroU32, Option<&'static str>)> {
        let root = self.thread.active_finding_root(user)?;
        if !review.wants_resolution(self.thread.id().get()) {
            return None;
        }
        if !self.anchor_changed(context)? {
            return None;
        }
        let reply = (!self.thread.has_reply(user, REPLY)).then_some(REPLY);
        Some((root, reply))
    }

    fn anchor_changed(&self, context: &Context) -> Option<bool> {
        let (path, start, end) = self.thread.anchor()?;
        context.has_anchor(path, (start, end)).then_some(())?;
        Some(self.fingerprint? != context.anchor_fingerprint(path, (start, end))?)
    }
}

fn parse_marker(line: &str) -> Option<&str> {
    const SHA256_HEX_LENGTH: usize = 64;
    let fingerprint = line.strip_prefix(PREFIX)?.strip_suffix(MARKER_END)?;
    (fingerprint.len() == SHA256_HEX_LENGTH
        && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(fingerprint)
}
