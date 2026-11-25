use rstest::rstest;

use super::Repository;

#[rstest]
#[case(
    "https://org@example.test/tfs/Team%20Collection/My%20Project/_git/My%20Repo",
    "https://example.test/tfs/Team%20Collection",
    (Some("My Project"), "My Repo")
)]
#[case(
    "https://example.test/Collection/_git/Repo",
    "https://example.test/Collection",
    (None, "Repo")
)]
#[case(
    "https://org@dev.azure.com/org/proj/_git/repo",
    "https://dev.azure.com/org",
    (Some("proj"), "repo")
)]
#[case(
    "https://org.visualstudio.com/proj/_git/repo",
    "https://org.visualstudio.com/",
    (Some("proj"), "repo")
)]
#[case(
    "https://org.visualstudio.com/DefaultCollection/proj/_git/repo",
    "https://org.visualstudio.com/DefaultCollection",
    (Some("proj"), "repo")
)]
fn parses_clone_urls(
    #[case] input: &str,
    #[case] collection: &str,
    #[case] scope: (Option<&str>, &str),
) {
    let repo = Repository::parse(input).unwrap();
    assert_eq!(repo.collection.as_str(), collection);
    assert_eq!((repo.project.as_deref(), repo.name.as_str()), scope);
    assert_eq!(repo.collection.username(), "");
}

#[test]
fn builds_project_scoped_rest_endpoint() {
    let repo = Repository::parse("https://org@example.test/tfs/Collection/My%20Project/_git/Repo")
        .unwrap();
    let endpoint = repo.git_api(&["pullRequests", "7", "iterations"]).unwrap();
    assert_eq!(
        endpoint.as_str(),
        "https://example.test/tfs/Collection/My%20Project/_apis/git/repositories/Repo/pullRequests/7/iterations?api-version=7.1"
    );
}

#[rstest]
#[case("https://user:example-secret@example.test/collection/project/_git/repo")]
#[case("https://user:@example.test/collection/project/_git/repo")]
#[case("https://example.test/collection/project/_git/repo/pullrequest/1")]
#[case("http://example.test/collection/project/_git/repo")]
#[case("https://example.test/collection/project/_git/a%2Fb")]
#[case("https://example.test/collection/project/_git/%FF")]
#[case("https://example.test/collection/project/_git/repo?secret=example-secret")]
#[case("https://example.test/tfs/_git/repo")]
fn rejects_invalid_urls_without_echoing_input(#[case] input: &str) {
    let error = Repository::parse(input).unwrap_err().to_string();
    assert!(!error.contains(input));
    assert!(!error.contains("example-secret"));
}
