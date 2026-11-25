use assert_cmd::cargo::cargo_bin_cmd;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use rstest::rstest;

#[rstest]
#[case(&["review"], "required")]
#[case(&["review", "--pr", "0"], "invalid value")]
#[case(&["review", "--effort", "high"], "invalid value")]
#[case(&["review", "--model", "custom"], "unexpected argument")]
fn reports_failure(#[case] args: &[&str], #[case] error: &str) {
    cargo_bin_cmd!()
        .args(args)
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains(error));
}

#[rstest]
#[case(&["--help"], "Usage:")]
#[case(&["review", "--help"], "adaptive, lite, balanced")]
#[case(&["--version"], env!("CARGO_PKG_VERSION"))]
fn prints_information(#[case] args: &[&str], #[case] text: &str) {
    cargo_bin_cmd!()
        .args(args)
        .assert()
        .success()
        .stdout(contains(text))
        .stderr("");
}

#[test]
fn does_not_echo_passwords_in_repository_urls() {
    cargo_bin_cmd!()
        .args([
            "review",
            "--repo-url",
            "https://user:example-secret@example.test/collection/project/_git/repo",
            "--pr",
            "1",
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("invalid --repo-url").and(contains("example-secret").not()));
}

#[test]
fn requires_an_azure_token_before_contacting_the_server() {
    cargo_bin_cmd!()
        .env_remove("AZURE_TOKEN")
        .args([
            "review",
            "--repo-url",
            "https://example.test/collection/project/_git/repo",
            "--pr",
            "1",
            "--dry-run",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr("missing or invalid environment variable: AZURE_TOKEN\n");
}
