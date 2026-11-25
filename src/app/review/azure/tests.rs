use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use super::{AzureError, Client, Repository, http};
use crate::app::review::context::{Context, Exclusions};

const PROJECT_ID: &str = "484468ed-d80e-4e51-9224-56602582169d";
const REPO_ID: &str = "37bbc168-e013-46d3-bc77-be6a0f338fdf";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn pull_request_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "pullRequestId": 1,
        "title": "Synthetic PR",
        "repository": {
            "id": REPO_ID,
            "name": "repository",
            "project": {"id": PROJECT_ID, "name": "Project"}
        }
    }))
}

#[tokio::test]
async fn resolves_the_authenticated_identity_at_collection_scope() {
    let identity = identity_response(PROJECT_ID).await.unwrap();
    assert_eq!(identity.to_string(), PROJECT_ID);
}

#[tokio::test]
async fn refuses_to_treat_the_anonymous_identity_as_owner() {
    let result = identity_response("00000000-0000-0000-0000-000000000000").await;
    assert!(matches!(result, Err(AzureError::Response)));
}

async fn identity_response(id: &str) -> Result<uuid::Uuid, AzureError> {
    let server = MockServer::start().await;
    let origin = Url::parse(&server.uri()).unwrap();
    let repository = Repository::for_mock(origin.join("/Collection").unwrap(), Some("Project"));
    Mock::given(method("GET"))
        .and(path("/Collection/_apis/connectionData"))
        .and(query_param("api-version", "7.1-preview.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser":{"id":id},"authorizedUser":{"id":REPO_ID}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut client = Client(http::Transport::new(&"sample".into()).unwrap());
    client.identity(&repository)
}

#[tokio::test]
async fn discovers_project_for_short_url_and_loads_latest_iteration() {
    let server = MockServer::start().await;
    let (client, snapshot) = load_snapshot(&server).await;
    drop(client);
    assert_eq!(snapshot.iteration_id().get(), 3);
    assert_eq!(snapshot.threads.len(), 2);
}

async fn load_snapshot(server: &MockServer) -> (Client, super::Snapshot) {
    let origin = Url::parse(&server.uri()).unwrap();
    let repository = Repository::for_mock(origin.join("/Collection").unwrap(), None);
    let mut client = Client(http::Transport::new(&"sample".into()).unwrap());
    Mock::given(method("GET"))
        .and(path("/Collection/_apis/git/pullRequests/1"))
        .and(query_param("api-version", "7.1"))
        .respond_with(pull_request_response())
        .expect(1)
        .mount(server)
        .await;

    let base = format!("/Collection/{PROJECT_ID}/_apis/git/repositories/{REPO_ID}/pullRequests/1");
    for (endpoint, json) in [
        (
            "/iterations",
            serde_json::json!({"value": [iteration(2), iteration(3), iteration(1)]}),
        ),
        (
            "/threads",
            serde_json::json!({"value":[{"id":5,"status":"active","comments":[]},{"id":9,"status":"closed","comments":[]}]}),
        ),
    ] {
        let mut request = Mock::given(method("GET"))
            .and(path(format!("{base}{endpoint}")))
            .and(query_param("api-version", "7.1"));
        if endpoint == "/threads" {
            request = request.and(query_param("$iteration", "3"));
        }
        request
            .respond_with(ResponseTemplate::new(200).set_body_json(json))
            .expect(1)
            .mount(server)
            .await;
    }

    let snapshot = client.snapshot(&repository, 1.try_into().unwrap()).unwrap();
    (client, snapshot)
}

fn iteration(id: u32) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "commonRefCommit":{"commitId":BASE},
        "sourceRefCommit":{"commitId":HEAD},
        "targetRefCommit":{"commitId":TARGET}
    })
}

#[tokio::test]
async fn gathers_changed_files_across_pages_with_commit_pinned_items() {
    let server = MockServer::start().await;
    let (mut client, snapshot) = load_snapshot(&server).await;
    mock_changes(&server).await;
    mock_items(&server).await;
    let context = Context::gather(&mut client, &snapshot, &Exclusions::new(&[]).unwrap()).unwrap();
    assert_eq!(context.counts(), (1, 1, 2));
    assert!(context.has_anchor("/src/lib.rs", (2, 2)));
    let value = serde_json::to_value(context).unwrap();
    assert_eq!(
        value["instructions"][0]["applies_to"],
        serde_json::json!(["src/lib.rs"])
    );
    assert_eq!(value["files"][0]["new_content"], "start\nnew\nend\n");
    assert_eq!(value["skipped"][0]["reason"], "default_excluded");
}

#[tokio::test]
async fn keeps_rename_and_delete_sides_and_ignores_directory_entries() {
    let server = MockServer::start().await;
    let (mut client, snapshot) = load_snapshot(&server).await;
    Mock::given(path(changes_path()))
        .and(query_param("$compareTo", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "changeEntries": [
                {"changeType":"rename, edit","item":{"path":"/new.rs"},"originalPath":"/old.rs"},
                {"changeType":"delete","item":{"path":"/gone.rs"}},
                {"changeType":"edit","item":{"path":"/src","gitObjectType":"tree"}}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let changes = client.files(&snapshot).unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].before.as_deref(), Some("/old.rs"));
    assert_eq!(changes[0].path, "/new.rs");
    assert!(changes[0].after);
    assert_eq!(changes[1].before.as_deref(), Some("/gone.rs"));
    assert!(!changes[1].after);
}

#[tokio::test]
async fn stops_on_a_repeated_changes_continuation() {
    let server = MockServer::start().await;
    let (mut client, snapshot) = load_snapshot(&server).await;
    Mock::given(path(changes_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "changeEntries": [], "nextSkip": 1
        })))
        .expect(2)
        .mount(&server)
        .await;
    assert!(matches!(
        client.files(&snapshot),
        Err(AzureError::InvalidChangesPage)
    ));
}

fn changes_path() -> String {
    format!(
        "/Collection/{PROJECT_ID}/_apis/git/repositories/{REPO_ID}/pullRequests/1/iterations/3/changes"
    )
}

async fn mock_changes(server: &MockServer) {
    let endpoint = changes_path();
    for (skip, entry, next) in [
        (
            "0",
            serde_json::json!({"item":{"path":"/src/lib.rs"},"changeType":"edit"}),
            1,
        ),
        (
            "1",
            serde_json::json!({"item":{"path":"/Cargo.lock"},"changeType":"add"}),
            0,
        ),
    ] {
        Mock::given(path(&endpoint))
            .and(query_param("$compareTo", "0"))
            .and(query_param("$skip", skip))
            .and(query_param("api-version", "7.1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "changeEntries": [entry], "nextSkip": next
            })))
            .expect(1)
            .mount(server)
            .await;
    }
}

async fn mock_items(server: &MockServer) {
    let endpoint = format!("/Collection/{PROJECT_ID}/_apis/git/repositories/{REPO_ID}/items");
    Mock::given(path(&endpoint))
        .and(query_param("scopePath", "/.github/instructions"))
        .and(query_param("versionDescriptor.version", TARGET))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value":[{"path":"/.github/instructions/rust.instructions.md"}]
        })))
        .expect(1)
        .mount(server)
        .await;
    for (file, commit, body) in [
        ("/src/lib.rs", BASE, Some("start\nold\nend\n")),
        ("/src/lib.rs", HEAD, Some("start\nnew\nend\n")),
        ("/.github/copilot-instructions.md", TARGET, None),
        (
            "/.github/instructions/rust.instructions.md",
            TARGET,
            Some("---\napplyTo: '**/*.{rs,toml}, **/*.md'\n---\nPrefer clear failures."),
        ),
    ] {
        Mock::given(path(&endpoint))
            .and(query_param("path", file))
            .and(query_param("versionDescriptor.version", commit))
            .and(query_param("versionDescriptor.versionType", "commit"))
            .and(wiremock::matchers::header(
                "Accept",
                "application/octet-stream",
            ))
            .respond_with(body.map_or_else(
                || ResponseTemplate::new(404),
                |body| ResponseTemplate::new(200).set_body_string(body),
            ))
            .expect(1)
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn rejects_pr_from_another_project() {
    let server = MockServer::start().await;
    let origin = Url::parse(&server.uri()).unwrap();
    let repo = Repository::for_mock(origin.join("/Collection").unwrap(), Some("Other"));
    let mut client = Client(http::Transport::new(&"sample".into()).unwrap());
    Mock::given(path("/Collection/Other/_apis/git/pullRequests/1"))
        .respond_with(pull_request_response())
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        client.snapshot(&repo, 1.try_into().unwrap()),
        Err(AzureError::WrongRepository)
    ));
}

#[tokio::test]
async fn refuses_to_review_a_pull_request_without_any_iterations() {
    let server = MockServer::start().await;
    let origin = Url::parse(&server.uri()).unwrap();
    let repo = Repository::for_mock(origin.join("/Collection").unwrap(), None);
    Mock::given(path("/Collection/_apis/git/pullRequests/1"))
        .respond_with(pull_request_response())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!(
        "/Collection/{PROJECT_ID}/_apis/git/repositories/{REPO_ID}/pullRequests/1/iterations"
    )))
    .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": []})))
    .expect(1)
    .mount(&server)
    .await;
    let mut client = Client(http::Transport::new(&"sample".into()).unwrap());
    assert!(matches!(
        client.snapshot(&repo, 1.try_into().unwrap()),
        Err(AzureError::MissingIteration)
    ));
}
