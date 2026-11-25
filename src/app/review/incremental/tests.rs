use serde_json::{Value, json};

use super::*;

const USER: Uuid = Uuid::from_u128(1);
const HUMAN: Uuid = Uuid::from_u128(2);

#[test]
fn only_the_token_identity_can_record_a_reviewed_iteration() {
    let comment = |number| format!("{SUMMARY}\n{RUN_PREFIX}{number}{MARKER_END}\nOverview");
    let threads = [
        thread("closed", (HUMAN, &comment(9)), None),
        thread("closed", (USER, &comment(3)), None),
        thread("closed", (USER, &format!("text\n{}", comment(7))), None),
    ];
    let previous = PreviousReview::from_threads(&threads, USER);
    assert!(previous.includes(3.try_into().unwrap()));
    assert!(!previous.includes(4.try_into().unwrap()));
}

#[test]
fn suppresses_only_overlapping_findings_handled_by_humans() {
    let finding = |path, line| {
        json!({
            "title":"Problem", "body":"Description.", "severity":"high",
            "location":{"path":path,"start":line,"end":line}, "suggestion":null
        })
    };
    let mut review: Review = serde_json::from_value(json!({
        "overview":"Review.", "existing":[],
        "new_findings":[finding("src/lib.rs", 5), finding("/src/lib.rs", 6), finding("src/main.rs", 5)]
    })).unwrap();
    let threads = [
        thread("active", (USER, FINDING), Some(HUMAN)),
        thread("closed", (HUMAN, FINDING), None),
        thread("active", (USER, FINDING), Some(USER)),
    ];
    PreviousReview::from_threads(&threads, USER).filter_findings(&mut review);
    assert_eq!(review.new_findings.len(), 2);
    assert_eq!(review.new_findings[0].location.as_ref().unwrap().start, 6);
}

fn thread(status: &str, (author, content): (Uuid, &str), reply: Option<Uuid>) -> Thread {
    let mut comments = vec![json!({"content":content,"author":{"id":author}})];
    comments.extend(reply.map(|id| json!({"author":{"id":id},"parentCommentId":1})));
    let value: Value = json!({
        "id":1, "status":status, "comments":comments,
        "threadContext":{"filePath":"/src/lib.rs","rightFileStart":{"line":4},"rightFileEnd":{"line":5}}
    });
    serde_json::from_value(value).unwrap()
}
