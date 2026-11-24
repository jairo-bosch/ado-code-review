use std::{
    fs, io,
    path::Path,
    process::{Command, Output},
};

const MANIFEST: &str = include_str!("../../Cargo.toml");

fn source(root: &Path) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("source")
        .current_dir(root)
        .output()
}

fn test_modules(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("src/nested"))?;
    fs::write(
        root.join("src/lib.rs"),
        r#"
        #[cfg(test)] mod test_fixtures;
        mod nested {
            #[cfg(test)]
            #[path = "helper.rs"]
            mod renamed;
        }
        fn ok() -> i32 { 1 }
    "#,
    )?;
    fs::write(
        root.join("src/test_fixtures.rs"),
        "fn ignored() -> i32 { 42 }",
    )?;
    fs::write(
        root.join("src/nested/helper.rs"),
        "fn ignored() -> i32 { 43 }",
    )
}

#[test]
fn checks_both_packages_and_reports_relative_source_location() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    fs::create_dir_all(root.join("xtask/src")).unwrap();
    test_modules(root).unwrap();
    fs::write(root.join("xtask/src/tests.rs"), "not even Rust").unwrap();
    assert!(source(root).unwrap().status.success());

    fs::write(root.join("xtask/src/main.rs"), "\nfn fails() -> i32 { 42 }").unwrap();
    let output = source(root).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let finding = String::from_utf8_lossy(&output.stdout);
    assert!(
        finding.contains("xtask/src/main.rs:2: magic-number"),
        "{finding}"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("1 source quality violations"), "{error}");
}

#[test]
fn rejects_parse_errors_and_missing_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    let missing = source(root).unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot walk ./src"));

    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "fn invalid(").unwrap();
    let invalid = source(root).unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("cannot parse src/lib.rs"));
}
