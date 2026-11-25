use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use syn::{Expr, ExprLit, Item, Lit};

const RUNTIME_SOURCE: &str = "src/app/review/copilot/runtime.rs";
const SDK_ASSET: &str = "copilot-linux-x64.tar.gz";
// The SDK pins the standalone CLI, not the npm release archive used by the adapter.
const SDK_SHA256: &str = "e01975bf3a73791b0c0adfdf0d7093b8616186db61b78b33604b124806808816";
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
}

pub(super) fn check(root: &Path) -> Result<()> {
    let runtime_source = fs::read_to_string(root.join(RUNTIME_SOURCE))?;
    let manifest = fs::read_to_string(sdk_manifest(root)?)?;
    verify(&runtime_source, &manifest)
}

fn sdk_manifest(root: &Path) -> Result<PathBuf> {
    let output = Command::new("cargo")
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("running cargo metadata for the runtime pin check")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed during the runtime pin check: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
    let mut sdk = metadata
        .packages
        .iter()
        .filter(|package| package.name == "copilot-sdk");
    let package = sdk.next().context("missing resolved copilot-sdk")?;
    ensure!(
        sdk.next().is_none(),
        "multiple resolved versions of copilot-sdk"
    );
    Ok(package.manifest_path.with_file_name("cli-version.txt"))
}

fn verify(source: &str, manifest: &str) -> Result<()> {
    let source = syn::parse_file(source)?;
    let version = string_constant(&source, "VERSION")?;
    let digest = string_constant(&source, "SHA256")?;
    ensure!(
        manifest_value(manifest, "version") == Some(version.as_str()),
        "Copilot runtime version does not match the resolved SDK"
    );
    ensure!(
        manifest_value(manifest, SDK_ASSET) == Some(SDK_SHA256),
        "Copilot SDK standalone archive checksum changed"
    );
    ensure!(
        digest.len() == SHA256_HEX_LENGTH && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Copilot npm release archive checksum is invalid"
    );
    Ok(())
}

fn string_constant(source: &syn::File, name: &str) -> Result<String> {
    source
        .items
        .iter()
        .find_map(|item| {
            let Item::Const(item) = item else {
                return None;
            };
            if item.ident != name {
                return None;
            }
            match item.expr.as_ref() {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) => Some(value.value()),
                _ => None,
            }
        })
        .with_context(|| format!("missing runtime pin constant {name}"))
}

fn manifest_value<'a>(manifest: &'a str, name: &str) -> Option<&'a str> {
    manifest
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

#[cfg(test)]
mod tests;
