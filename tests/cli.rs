use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use rstest::rstest;

#[rstest]
#[case(["add", "-3", "-2"], "-5\n")]
#[case(["add", "-1e-2", "2"], "1.99\n")]
#[case(["subtract", "3", "2"], "1\n")]
#[case(["multiply", "3", "2"], "6\n")]
#[case(["divide", "7", "2"], "3.5\n")]
fn prints_answer(#[case] args: [&str; 3], #[case] answer: &'static str) {
    cargo_bin_cmd!()
        .args(args)
        .assert()
        .success()
        .stdout(answer)
        .stderr("");
}

#[rstest]
#[case(&["divide", "1", "0"], 1, "division by zero")]
#[case(&["add", "NaN", "1"], 1, "operands must be finite")]
#[case(&["add", "1", "-inf"], 1, "operands must be finite")]
#[case(&["multiply", "1e308", "2"], 1, "result is not finite")]
#[case(&["add", "one", "2"], 2, "invalid value")]
#[case(&["add", "1"], 2, "required")]
#[case(&["add", "1", "2", "3"], 2, "unexpected argument")]
fn reports_failure(#[case] args: &[&str], #[case] status: i32, #[case] error: &str) {
    cargo_bin_cmd!()
        .args(args)
        .assert()
        .code(status)
        .stdout("")
        .stderr(contains(error));
}

#[rstest]
#[case(&["--help"], "Usage:")]
#[case(&["divide", "--help"], "<LEFT> <RIGHT>")]
#[case(&["--version"], env!("CARGO_PKG_VERSION"))]
fn prints_information(#[case] args: &[&str], #[case] text: &str) {
    cargo_bin_cmd!()
        .args(args)
        .assert()
        .success()
        .stdout(contains(text))
        .stderr("");
}
