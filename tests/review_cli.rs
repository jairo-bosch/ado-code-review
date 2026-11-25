use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::{prelude::PredicateBooleanExt, str::contains};

struct Server {
    process: Child,
    port: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.process.kill());
        drop(self.process.wait());
    }
}

#[test]
fn skips_reviewed_iterations_without_copilot_but_force_runs_the_read_only_preview() {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, "preview");
    let review = || {
        let mut command = review_command(&server.port, temp.path(), &cert);
        command.arg("--dry-run");
        command
    };
    review()
        .env_remove("COPILOT_GITHUB_TOKEN")
        .assert()
        .success()
        .stdout("")
        .stderr(contains("Iteration 3 has already been reviewed; skipping"));

    assert_preview_failures(&review);
    install_fixture_runtime(temp.path());
    review()
        .arg("--force")
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .success()
        .stdout(contains("Synthetic review is complete."))
        .stderr(contains("Loaded iteration 3: 1 changed files"));

    install_fixture_runtime(&temp.path().join(".cache"));
    review()
        .arg("--force")
        .env("XDG_CACHE_HOME", "relative-cache")
        .env("HOME", temp.path())
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .success()
        .stdout(contains("Synthetic review is complete."));
    assert!(!temp.path().join("writes.jsonl").exists());
}

#[rstest::rstest]
#[case("history", "Previously missed")]
#[case("resolve", "Resolved since last review")]
#[case("unchanged", "Still open")]
#[case("missing-anchor", "**Other findings**")]
fn previews_incremental_results_without_writes(#[case] scenario: &str, #[case] section: &str) {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, scenario);
    install_fixture_runtime(temp.path());
    review_command(&server.port, temp.path(), &cert)
        .arg("--dry-run")
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .success()
        .stdout(contains(section));
    assert!(!temp.path().join("writes.jsonl").exists());
}

#[test]
#[ignore = "requires live Copilot credentials and network; Azure and PR content are synthetic"]
fn previews_synthetic_pr_with_live_copilot() {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, "create");
    let trust = std::env::var_os("SSL_CERT_FILE")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || PathBuf::from("/etc/ssl/certs/ca-certificates.crt"),
            PathBuf::from,
        );
    let bundle = temp.path().join("bundle.pem");
    fs::write(
        &bundle,
        format!(
            "{}\n{}",
            fs::read_to_string(trust).unwrap(),
            fs::read_to_string(&cert).unwrap()
        ),
    )
    .unwrap();
    review_command(&server.port, temp.path(), &bundle)
        .args(["--dry-run", "--effort", "lite"])
        .assert()
        .success()
        .stdout(contains("_Review effort: Lite"));
    assert!(!temp.path().join("writes.jsonl").exists());
}

#[rstest::rstest]
#[case("create", "Published review", 0)]
#[case("update", "Published review", 0)]
#[case("wrong-update", "Azure DevOps returned an invalid response", 1)]
#[case("history", "Published review", 0)]
#[case("history-missing", "Published review", 0)]
#[case("history-id-missing", "Published review", 0)]
#[case("stale", "nothing was published", 0)]
#[case("stale-after-history", "nothing was published", 0)]
#[case("concurrent", "Iteration 3 has already been reviewed; skipping", 0)]
#[case("protected", "the existing review summary was changed", 1)]
#[case("failure", "HTTP 503", 1)]
fn controls_publication(#[case] scenario: &str, #[case] message: &str, #[case] code: i32) {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, scenario);
    install_fixture_runtime(temp.path());
    let mut command = review_command(&server.port, temp.path(), &cert);
    if matches!(scenario, "update" | "wrong-update" | "protected") {
        command.arg("--force");
    }
    command
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .code(code)
        .stdout("")
        .stderr(contains(message));
    let log = temp.path().join("writes.jsonl");
    if matches!(
        scenario,
        "stale" | "stale-after-history" | "concurrent" | "protected"
    ) {
        assert!(!log.exists());
        return;
    }
    let requests = recorded_writes(log);
    assert_publication_requests(scenario, &requests);
}

fn assert_publication_requests(scenario: &str, requests: &[serde_json::Value]) {
    let [inline, rest @ ..] = requests else {
        panic!("no inline request");
    };
    assert_eq!(inline[0], "POST");
    assert_eq!(inline[2]["status"], "active");
    assert_eq!(
        inline[2]["pullRequestThreadContext"]["changeTrackingId"],
        19
    );
    assert_eq!(
        inline[2]["threadContext"]["rightFileEnd"],
        serde_json::json!({"line": 1, "offset": 4})
    );
    if scenario == "failure" {
        assert_eq!(rest, [] as [serde_json::Value; 0]);
        return;
    }
    let [summary] = rest else {
        panic!("expected one summary request");
    };
    let markdown = if scenario.ends_with("update") || scenario.starts_with("history") {
        assert_eq!(summary[0], "PATCH");
        summary[2]["content"].as_str().unwrap()
    } else {
        assert_eq!(summary[0], "POST");
        assert_eq!(summary[2]["status"], "closed");
        summary[2]["comments"][0]["content"].as_str().unwrap()
    };
    assert!(
        markdown.starts_with(
            "<!-- ado-code-review:summary:v1 -->\n<!-- ado-code-review:iteration:3 -->"
        )
    );
    assert!(markdown.contains("discussionId=42"));
    if scenario == "history" {
        assert!(markdown.contains("Previously missed"));
    }
    if matches!(scenario, "history-missing" | "history-id-missing") {
        assert!(markdown.contains(" · New"));
    }
}

#[test]
fn retries_after_partial_publication_without_reposting_the_existing_inline() {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, "partial");
    install_fixture_runtime(temp.path());
    for (code, message) in [(1, "HTTP 503"), (0, "Published review")] {
        review_command(&server.port, temp.path(), &cert)
            .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
            .env("COPILOT_GH_HOST", "GitHub.Example")
            .assert()
            .code(code)
            .stderr(contains(message));
    }
    let requests = recorded_writes(temp.path().join("writes.jsonl"));
    let [inline, failed_summary, summary] = requests.as_slice() else {
        panic!("expected one inline and two summaries");
    };
    assert_eq!(inline[2]["status"], "active");
    assert_eq!(summary_markdown(failed_summary), summary_markdown(summary));
    assert!(summary_markdown(summary).contains("discussionId=42"));
}

#[test]
fn puts_untrackable_findings_in_the_summary_without_an_inline_suggestion() {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, "missing-anchor");
    install_fixture_runtime(temp.path());
    review_command(&server.port, temp.path(), &cert)
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .success();
    let requests = recorded_writes(temp.path().join("writes.jsonl"));
    let [summary] = requests.as_slice() else {
        panic!("expected one summary");
    };
    assert!(summary_markdown(summary).contains("**Other findings**"));
    assert!(!summary_markdown(summary).contains("```suggestion"));
}

#[rstest::rstest]
#[case("resolve")]
#[case("unchanged")]
#[case("human")]
#[case("bad-marker")]
#[case("unmapped-anchor")]
fn only_resolves_owned_unanswered_findings_with_changed_lines(#[case] scenario: &str) {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, scenario);
    install_fixture_runtime(temp.path());
    review_command(&server.port, temp.path(), &cert)
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "GitHub.Example")
        .assert()
        .success()
        .stderr(contains("Published review"));
    let requests = recorded_writes(temp.path().join("writes.jsonl"));
    if scenario == "resolve" {
        let [reply, status, summary] = requests.as_slice() else {
            panic!("expected reply, resolution and summary");
        };
        assert_eq!(reply[0], "POST");
        assert_eq!(reply[2]["parentCommentId"], 9);
        assert_eq!(status[0], "PATCH");
        assert_eq!(status[2]["status"], "fixed");
        assert!(summary_markdown(summary).contains("Resolved since last review"));
    } else {
        let [summary] = requests.as_slice() else {
            panic!("expected only a summary");
        };
        assert!(summary_markdown(summary).contains("Still open"));
    }
}

fn summary_markdown(request: &serde_json::Value) -> &str {
    assert_eq!(request[0], "POST");
    assert_eq!(request[2]["status"], "closed");
    request[2]["comments"][0]["content"].as_str().unwrap()
}

fn recorded_writes(log: PathBuf) -> Vec<serde_json::Value> {
    fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn review_command(port: &str, cache: &Path, cert: &Path) -> assert_cmd::Command {
    let repository = format!("https://127.0.0.1:{port}/Collection/Project/_git/Repository");
    let mut command = cargo_bin_cmd!();
    command
        .args(["review", "--repo-url", &repository, "--pr", "1"])
        .env("SSL_CERT_FILE", cert)
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env("AZURE_TOKEN", "synthetic-azure")
        .env("XDG_CACHE_HOME", cache);
    command
}

fn assert_preview_failures(review: &impl Fn() -> assert_cmd::Command) {
    review()
        .arg("--force")
        .env_remove("COPILOT_GITHUB_TOKEN")
        .assert()
        .code(1)
        .stderr(contains(
            "missing or invalid environment variable: COPILOT_GITHUB_TOKEN",
        ));
    review()
        .arg("--force")
        .env("COPILOT_GITHUB_TOKEN", "$(UnexpandedSecret)")
        .assert()
        .code(1)
        .stderr(
            contains("missing or invalid environment variable: COPILOT_GITHUB_TOKEN")
                .and(contains("UnexpandedSecret").not()),
        );
    review()
        .arg("--force")
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env("COPILOT_GH_HOST", "https://github.example")
        .assert()
        .code(1)
        .stderr(contains("COPILOT_GH_HOST must be a bare host"));
    review()
        .args(["--exclude", "["])
        .assert()
        .code(2)
        .stderr(contains("invalid --exclude glob"));
    review()
        .env_remove("SSL_CERT_FILE")
        .assert()
        .code(1)
        .stderr(contains("could not connect to Azure DevOps"));
}

#[test]
fn reports_an_unavailable_runtime_cache_before_running_copilot() {
    let temp = tempfile::tempdir().unwrap();
    let cert = fixture_certificate(temp.path());
    let server = start_server(temp.path(), &cert, "preview");
    review_command(&server.port, temp.path(), &cert)
        .args(["--dry-run", "--force"])
        .env("XDG_CACHE_HOME", "relative-cache")
        .env_remove("HOME")
        .env("COPILOT_GITHUB_TOKEN", "synthetic-token")
        .env_remove("COPILOT_GH_HOST")
        .assert()
        .code(1)
        .stderr(contains("could not locate the Copilot runtime cache"));
}

fn fixture_certificate(root: &Path) -> PathBuf {
    let cert = root.join("cert.pem");
    let output = Command::new("openssl")
        .args([
            "req",
            "-new",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
            "-out",
        ])
        .arg(&cert)
        .arg("-keyout")
        .arg(root.join("key.pem"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    cert
}

fn start_server(root: &Path, cert: &Path, scenario: &str) -> Server {
    let process = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/azure_review.py"
        ))
        .arg(cert)
        .arg(root.join("key.pem"))
        .arg(scenario)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Server {
        process,
        port: String::new(),
    };
    BufReader::new(server.process.stdout.as_mut().unwrap())
        .read_line(&mut server.port)
        .unwrap();
    server.port.truncate(server.port.trim_end().len());
    assert!(server.port.parse::<u16>().is_ok());
    server
}

fn install_fixture_runtime(root: &Path) {
    let destination = root.join(
        "ado-code-review/copilot-runtime/1.0.115/package/prebuilds/linux-x64/copilot-runtime",
    );
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, include_bytes!("fixtures/copilot_runtime.py")).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
}
