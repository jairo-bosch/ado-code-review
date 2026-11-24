use std::{
    collections::BTreeSet,
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
    source::{module_path, production, test_only},
    test_modules,
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
    let excluded = excluded_modules(&files);
    let mut findings = Vec::new();
    for source in files {
        if !source
            .path
            .ancestors()
            .any(|ancestor| excluded.contains(ancestor))
        {
            let mut module = vec![directory.to_string_lossy().into_owned()];
            module.extend(module_path(&source.path));
            let parsed = (directory.join(source.path), source.lines, source.ast?);
            findings.extend(file(parsed, quality, (module, methods)));
        }
    }
    Ok(findings)
}

fn excluded_modules(files: &[Source]) -> BTreeSet<PathBuf> {
    let mut excluded = BTreeSet::new();
    for source in files {
        if let Ok(ast) = &source.ast {
            test_modules::exclusions(&source.path, ast, &mut excluded);
        }
    }
    excluded
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
        if entry.file_type().is_file() && production(entry.path()) {
            files.push(parse(entry.path(), &source_root, directory)?);
        }
    }
    Ok(files)
}

fn parse(absolute: &Path, source_root: &Path, directory: &Path) -> anyhow::Result<Source> {
    let path = absolute.strip_prefix(source_root)?.to_path_buf();
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
    (path, lines, ast): (PathBuf, usize, syn::File),
    quality: &Quality,
    scopes: (Vec<String>, &mut Methods),
) -> Vec<Finding> {
    let (module, methods) = scopes;
    let mut report = Report {
        path,
        findings: Vec::new(),
    };
    if test_only(&ast.attrs) {
        return report.findings;
    }
    report.limit(
        (Span::call_site(), "file-lines"),
        (lines, quality.size.file_lines),
    );
    let mut check = Check {
        quality,
        report,
        methods,
        module,
    };
    check.visit_file(&ast);
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
