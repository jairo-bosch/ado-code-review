use std::{collections::VecDeque, future, sync::Mutex};
use std::{fs, os::unix::fs::PermissionsExt};

use super::*;

struct Fake {
    responses: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<String>>,
}

impl Conversation for Fake {
    fn exchange(
        &self,
        prompt: &str,
        schema: Value,
    ) -> impl Future<Output = Result<String, CopilotError>> + Send {
        assert_eq!(schema["additionalProperties"], false);
        self.prompts.lock().unwrap().push(prompt.into());
        future::ready(
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(CopilotError::Request),
        )
    }
}

#[rstest::rstest]
#[case(false, "changed new-side lines")]
#[case(true, "expected a schema-conforming JSON object")]
#[tokio::test]
async fn retries_invalid_output(#[case] invalid_json: bool, #[case] diagnostic: &str) {
    let report = |line: usize| {
        json!({
            "overview": "Review of the changes.",
            "new_findings": [{
                "title": "Synthetic issue",
                "body": "Describe the consequence.",
                "severity": "medium",
                "location": { "path":"src/lib.rs", "start":line, "end":line },
                "suggestion": null
            }],
            "existing": [{"id":4,"status":"still_open"}]
        })
        .to_string()
    };
    let first = if invalid_json {
        "not JSON".into()
    } else {
        report(1)
    };
    let fake = Fake {
        responses: Mutex::new(VecDeque::from([first, report(2)])),
        prompts: Mutex::default(),
    };
    let context = crate::app::review::context::fixture();
    let review = review_with_retry(&fake, "synthetic input", &context)
        .await
        .unwrap();
    assert_eq!(review.new_findings.len(), 1);
    let prompts = fake.prompts.into_inner().unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].contains(diagnostic));
}

#[tokio::test]
async fn rejects_unknown_discussion_ids_after_one_retry() {
    let response =
        r#"{"overview":"No issue","new_findings":[],"existing":[{"id":999,"status":"resolved"}]}"#;
    let fake = Fake {
        responses: Mutex::new(VecDeque::from([
            response.into(),
            response.into(),
            response.into(),
        ])),
        prompts: Mutex::default(),
    };
    let context = crate::app::review::context::fixture();
    let result = review_with_retry(&fake, "synthetic input", &context).await;
    assert!(matches!(result, Err(CopilotError::Invalid(_))));
    assert_eq!(fake.prompts.into_inner().unwrap().len(), 2);
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn runs_adaptive_review_over_isolated_stdio(#[case] blank_reason: bool) {
    let temp = tempfile::tempdir().unwrap();
    let binary = install_fixture(temp.path());
    if blank_reason {
        fs::write(temp.path().join("blank-triage-reason"), b"").unwrap();
    }
    let context = crate::app::review::context::fixture();
    let mut progress = Vec::new();
    let invocation = Invocation {
        effort: Effort::Adaptive,
        home: temp.path(),
        progress: &mut progress,
    };
    let options = Credentials::fixture().options(&binary, temp.path());
    let result = drive(options, &context, invocation).await;
    if blank_reason {
        assert!(matches!(
            result,
            Err(CopilotError::Invalid("adaptive effort requires a reason"))
        ));
        return;
    }
    let (review, mode) = result.unwrap_or_else(|error| {
        panic!(
            "{error}: {}",
            fs::read_to_string(temp.path().join("fixture-error")).unwrap_or_default()
        )
    });
    assert_eq!(review.overview, "Synthetic review is complete.");
    assert!(matches!(mode, Mode::Balanced));
    assert!(String::from_utf8(progress).unwrap().contains(mode.model()));
}

#[rstest::rstest]
#[case(Effort::Adaptive)]
#[case(Effort::Lite)]
#[tokio::test]
async fn requires_only_the_models_used_by_the_requested_effort(#[case] effort: Effort) {
    let temp = tempfile::tempdir().unwrap();
    let binary = install_fixture(temp.path());
    fs::write(temp.path().join("omit-balanced-model"), b"").unwrap();
    let context = crate::app::review::context::fixture();
    let mut progress = Vec::new();
    let invocation = Invocation {
        effort,
        home: temp.path(),
        progress: &mut progress,
    };
    let options = Credentials::fixture().options(&binary, temp.path());
    let result = drive(options, &context, invocation).await;
    if effort == Effort::Adaptive {
        assert!(matches!(
            result,
            Err(CopilotError::Model("claude-sonnet-5.5"))
        ));
        assert_eq!(progress, [] as [u8; 0]);
    } else {
        assert!(matches!(result, Ok((_, Mode::Lite))));
    }
}

fn install_fixture(home: &Path) -> std::path::PathBuf {
    let binary = home.join("copilot-runtime");
    fs::write(
        &binary,
        include_str!("../../../../tests/fixtures/copilot_runtime.py"),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    binary
}

#[tokio::test]
async fn does_not_start_a_review_if_the_runtime_cannot_be_executed() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("not-executable");
    fs::write(&binary, b"incomplete runtime").unwrap();
    let context = crate::app::review::context::fixture();
    let mut progress = Vec::new();
    let invocation = Invocation {
        effort: Effort::Balanced,
        home: temp.path(),
        progress: &mut progress,
    };
    let options = Credentials::fixture().options(&binary, temp.path());
    assert!(matches!(
        drive(options, &context, invocation).await,
        Err(CopilotError::Startup)
    ));
    assert_eq!(progress, [] as [u8; 0]);
}

#[tokio::test]
async fn hides_runtime_rpc_diagnostics_when_model_listing_fails() {
    let temp = tempfile::tempdir().unwrap();
    let binary = install_fixture(temp.path());
    fs::write(temp.path().join("reject-catalog"), b"").unwrap();
    let context = crate::app::review::context::fixture();
    let mut progress = Vec::new();
    let invocation = Invocation {
        effort: Effort::Balanced,
        home: temp.path(),
        progress: &mut progress,
    };
    let options = Credentials::fixture().options(&binary, temp.path());
    let error = drive(options, &context, invocation).await.err().unwrap();
    assert_eq!(error.to_string(), "could not list available Copilot models");
    assert_eq!(progress, [] as [u8; 0]);
}
