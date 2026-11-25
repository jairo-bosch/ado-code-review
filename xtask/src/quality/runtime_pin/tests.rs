use super::*;

#[test]
fn separates_the_adapter_archive_from_the_sdk_archive() {
    let source = format!(
        r#"const VERSION: &str = "1.2.3"; const SHA256: &str = "{}";"#,
        "a".repeat(64)
    );
    let manifest = format!("version=1.2.3\n{SDK_ASSET}={SDK_SHA256}\n");
    verify(&source, &manifest).unwrap();
    assert!(verify(&source, &manifest.replace("1.2.3", "2.0.0")).is_err());
    assert!(verify(&source, &manifest.replace(SDK_SHA256, "unexpected")).is_err());
    let malformed = source.replace(&"a".repeat(64), "unverified");
    assert!(verify(&malformed, &manifest).is_err());
}

#[test]
fn rejects_runtime_pins_that_are_not_string_constants() {
    let manifest = format!("version=1.2.3\n{SDK_ASSET}={SDK_SHA256}\n");
    let result = verify(
        r#"fn helper() {} const VERSION: u32 = 1; const SHA256: &str = "not-a-pin";"#,
        &manifest,
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "missing runtime pin constant VERSION"
    );
}
