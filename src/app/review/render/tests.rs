use super::*;
use serde_json::json;

#[test]
fn renders_a_dry_run_without_thread_links() {
    let context = crate::app::review::context::fixture();
    let review = sample_review();
    let mut out = Vec::new();
    preview(
        &mut out,
        &context,
        (
            &review,
            (Effort::Adaptive, Mode::Balanced),
            &Publication::default(),
        ),
    )
    .unwrap();
    insta::assert_snapshot!(String::from_utf8(out).unwrap(), @r###"
    ### 🔴 Changes required

    The PR changes the example.

    **1 open issues**
    - 🔴 **High** · Report invalid input · `src/lib.rs:2` · New

    **Other findings**

    🟡 **Low** · Handle deleted configuration

    &lt;script&gt;Not HTML.&lt;/script&gt;

    **Skipped files**

    | File | Reason |
    | --- | --- |
    | `Cargo.lock` | Default exclusion |

    ---
    _Review effort: Adaptive → Balanced · claude-sonnet-5.5_

    ---
    🔴 **High** · Report invalid input · `src/lib.rs:2`

    A malformed input reaches the parser.

    ```suggestion
    return Err(error);
    ```
    "###);
}

#[test]
fn renders_a_multiline_anchor_without_an_embedded_suggestion_fence() {
    let context = crate::app::review::context::fixture();
    let review: Review = serde_json::from_value(json!({
        "overview": "This change needs attention.",
        "new_findings": [{
            "title": "Handle empty input",
            "body": "The caller can pass empty input.",
            "severity": "medium",
            "location": { "path": "/src/lib.rs", "start": 2, "end": 3 },
            "suggestion": "```\nreturn Err(error);\n```"
        }],
        "existing": []
    }))
    .unwrap();
    let mut out = Vec::new();
    preview(
        &mut out,
        &context,
        (&review, (Effort::Lite, Mode::Lite), &Publication::default()),
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("Changes recommended"));
    assert!(text.contains("`src/lib.rs:2-3`"));
    assert!(text.contains("Review effort: Lite"));
    assert!(!text.contains("```"));
}

fn sample_review() -> Review {
    serde_json::from_value(serde_json::json!({
        "overview": "The PR changes the example.",
        "new_findings": [
            {
                "title": "Report invalid input",
                "body": "A malformed input reaches the parser.",
                "severity": "high",
                "location":{"path": "src/lib.rs", "start": 2, "end": 2},
                "suggestion": "return Err(error);"
            },
            {
                "title": "Handle deleted configuration",
                "body": "<script>Not HTML.</script>",
                "severity": "low",
                "location": null,
                "suggestion": null
            }
        ],
        "existing": []
    }))
    .unwrap()
}
