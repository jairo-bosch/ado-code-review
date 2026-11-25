use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use proc_macro2::Span;
use syn::visit::Visit;
use walkdir::WalkDir;

use super::{
    check::{Check, Methods},
    config::Quality,
    findings::{Finding, Report},
    modules,
    source::{module_path, production, test_only},
};

pub(super) fn workspace(root: &Path, quality: &Quality) -> anyhow::Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let mut methods = Methods::new();
    for package in ["", "xtask"] {
        let source_root = Path::new(package).join("src");
        findings.extend(package_files(root, &source_root, (quality, &mut methods))?);
    }
    findings.extend(public_methods(methods, quality));
    findings.retain(|finding| finding.applies(quality));
    Ok(findings)
}

fn package_files(
    root: &Path,
    directory: &Path,
    (quality, methods): (&Quality, &mut Methods),
) -> anyhow::Result<Vec<Finding>> {
    let files = scan(root, directory)?;
    let scopes = modules::scopes(
        directory,
        &files
            .iter()
            .map(|source| (source.path.as_path(), source.ast.as_ref().ok()))
            .collect(),
    );
    let mut findings = Vec::new();
    for source in files.into_iter().filter(|source| production(&source.path)) {
        findings.extend(check_source(
            source,
            (directory, &scopes),
            (quality, methods),
        )?);
    }
    Ok(findings)
}

fn check_source(
    source: Source,
    (directory, scopes): (&Path, &modules::Scopes),
    (quality, methods): (&Quality, &mut Methods),
) -> anyhow::Result<Vec<Finding>> {
    let Some(scope) = scopes.get(&source.path) else {
        let mut scope = vec![directory.to_string_lossy().into_owned()];
        scope.extend(module_path(&source.path));
        let parsed = (directory.join(source.path), source.lines, source.ast?);
        return Ok(file(&parsed, quality, (scope, methods)));
    };
    check_reached(source, (directory, scope), (quality, methods))
}

fn check_reached(
    source: Source,
    (directory, scope): (&Path, &BTreeSet<Vec<String>>),
    (quality, methods): (&Quality, &mut Methods),
) -> anyhow::Result<Vec<Finding>> {
    let mut routes = scope.iter();
    let Some(first) = routes.next() else {
        return Ok(Vec::new());
    };
    let parsed = (directory.join(source.path), source.lines, source.ast?);
    let findings = file(&parsed, quality, (first.clone(), methods));
    // Collect methods in every production scope, but report file findings only once.
    for module in routes {
        file(&parsed, quality, (module.clone(), methods));
    }
    Ok(findings)
}

struct Source {
    path: PathBuf,
    lines: usize,
    ast: anyhow::Result<syn::File>,
}

fn scan(root: &Path, directory: &Path) -> anyhow::Result<Vec<Source>> {
    let source_root = root.join(directory);
    let mut files = Vec::new();
    for entry in WalkDir::new(&source_root).sort_by_file_name() {
        let entry = entry.with_context(|| format!("cannot walk {}", source_root.display()))?;
        if entry.file_type().is_file() && entry.path().extension() == Some(OsStr::new("rs")) {
            files.push(parse(entry.path(), &source_root, directory)?);
        }
    }
    Ok(files)
}

fn parse(absolute: &Path, source_root: &Path, directory: &Path) -> anyhow::Result<Source> {
    let path = modules::normalize(absolute.strip_prefix(source_root)?)
        .with_context(|| format!("invalid source path {}", absolute.display()))?;
    let content = fs::read_to_string(absolute)
        .with_context(|| format!("cannot read {}", absolute.display()))?;
    let ast = syn::parse_file(&content)
        .with_context(|| format!("cannot parse {}", directory.join(&path).display()));
    Ok(Source {
        path,
        lines: content.lines().count(),
        ast,
    })
}

pub(super) fn file(
    (path, lines, ast): &(PathBuf, usize, syn::File),
    quality: &Quality,
    scopes: (Vec<String>, &mut Methods),
) -> Vec<Finding> {
    let (module, methods) = scopes;
    let mut report = Report {
        path: path.clone(),
        findings: Vec::new(),
    };
    if test_only(&ast.attrs) {
        return report.findings;
    }
    report.limit(
        (Span::call_site(), "file-lines"),
        (*lines, quality.size.file_lines),
    );
    let mut check = Check {
        quality,
        report,
        methods,
        module,
    };
    check.visit_file(ast);
    check.report.findings
}

pub(super) fn public_methods(methods: Methods, quality: &Quality) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (path, span, count) in methods.into_values() {
        let mut report = Report {
            path,
            findings: Vec::new(),
        };
        report.limit(
            (span, "public-methods"),
            (count, quality.size.public_methods),
        );
        findings.extend(report.findings);
    }
    findings
}
