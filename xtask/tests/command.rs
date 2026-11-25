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

fn workspace(root: &Path) -> io::Result<()> {
    fs::write(root.join("Cargo.toml"), MANIFEST)?;
    fs::create_dir_all(root.join("xtask/src"))?;
    fs::create_dir(root.join("src"))
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

#[test]
fn separates_crate_targets_but_aggregates_methods_across_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workspace(root).unwrap();
    let target = "
        struct Sample;
        impl Sample {
            pub fn a() {} pub fn b() {} pub fn c() {}
            pub fn d() {} pub fn e() {} pub fn f() {}
        }
    ";
    fs::write(root.join("src/lib.rs"), format!("{target} mod shared;")).unwrap();
    fs::write(root.join("src/main.rs"), target).unwrap();
    fs::write(root.join("src/shared.rs"), "").unwrap();
    assert!(source(root).unwrap().status.success());

    fs::write(
        root.join("src/shared.rs"),
        "impl crate::Sample {
            pub fn g() {} pub fn h() {} pub fn i() {}
            pub fn j() {} pub fn k() {}
        }",
    )
    .unwrap();
    let output = source(root).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let findings = String::from_utf8_lossy(&output.stdout);
    assert_eq!(findings.lines().count(), 1, "{findings}");
    assert!(
        findings.contains("src/lib.rs:3: public-methods"),
        "{findings}"
    );
}

fn module_tree(root: &Path) -> io::Result<()> {
    for dir in ["src/fixtures", "src/nested"] {
        fs::create_dir(root.join(dir))?;
    }
    fs::write(
        root.join("src/lib.rs"),
        r#"
        #[cfg(test)] #[path = "fixtures/mod.rs"] mod fixtures;
        #[path = "nested/module.rs"] mod nested;
        #[path = "fixtures/active.rs"] mod active;
        "#,
    )?;
    fs::write(root.join("src/fixtures/mod.rs"), "mod data; mod active;")?;
    fs::write(root.join("src/fixtures/data.rs"), "fn invalid(")?;
    fs::write(
        root.join("src/fixtures/active.rs"),
        "fn active() { use_value(33); }",
    )?;
    fs::write(
        root.join("src/nested/module.rs"),
        r#"#[cfg(test)] #[path = "../helper.rs"] mod helper;"#,
    )?;
    fs::write(root.join("src/helper.rs"), "fn helper() { use_value(42); }")?;
    fs::write(
        root.join("src/unlinked.rs"),
        "fn unlinked() { use_value(43); }",
    )
}

#[test]
fn excludes_only_test_module_files_after_resolving_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workspace(root).unwrap();
    module_tree(root).unwrap();
    let output = source(root).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let findings = String::from_utf8_lossy(&output.stdout);
    assert_eq!(findings.lines().count(), 2, "{findings}");
    assert!(
        findings.contains("src/fixtures/active.rs:1: magic-number"),
        "{findings}"
    );
    assert!(
        findings.contains("src/unlinked.rs:1: magic-number"),
        "{findings}"
    );
}
