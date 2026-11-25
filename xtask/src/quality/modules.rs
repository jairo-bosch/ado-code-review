use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use syn::{Expr, ExprLit, Item, Lit, Meta};

use super::source::test_only;

type Files<'a> = BTreeMap<&'a Path, Option<&'a syn::File>>;
pub(super) type Scopes = BTreeMap<PathBuf, BTreeSet<Vec<String>>>;

pub(super) fn scopes(directory: &Path, files: &Files<'_>) -> Scopes {
    let mut graph = Graph {
        files,
        scopes: Scopes::new(),
        active: BTreeSet::new(),
    };
    for path in files.keys().copied().filter(|path| crate_root(path)) {
        let scope = vec![directory.join(path).to_string_lossy().into_owned()];
        graph.file((path, parent(path)), (&scope, false));
    }
    graph.scopes
}

fn crate_root(path: &Path) -> bool {
    let dir = parent(path);
    match dir.to_str() {
        Some("") => matches!(path.to_str(), Some("lib.rs" | "main.rs")),
        Some("bin") => true,
        _ => parent(dir) == Path::new("bin") && path.ends_with("main.rs"),
    }
}

struct Graph<'a> {
    files: &'a Files<'a>,
    scopes: Scopes,
    active: BTreeSet<PathBuf>,
}

#[derive(Clone, Copy)]
struct Context<'a> {
    directory: &'a Path,
    custom_base: &'a Path,
    scope: &'a [String],
    test: bool,
}

impl Graph<'_> {
    fn file(&mut self, (path, directory): (&Path, &Path), (scope, test): (&[String], bool)) {
        if !self.active.insert(path.to_path_buf()) {
            return;
        }
        let ast = self.files.get(path).copied().flatten();
        let test = test || ast.is_some_and(|ast| test_only(&ast.attrs));
        let reached = self.scopes.entry(path.to_path_buf()).or_default();
        if !test {
            reached.insert(scope.to_vec());
        }
        if let Some(ast) = ast {
            self.items(
                &ast.items,
                Context {
                    directory,
                    custom_base: parent(path),
                    scope,
                    test,
                },
            );
        }
        self.active.remove(path);
    }

    fn items(&mut self, items: &[Item], context: Context<'_>) {
        for item in items {
            if let Item::Mod(module) = item {
                let mut child = context.scope.to_vec();
                child.push(module.ident.to_string());
                self.module(
                    module,
                    Context {
                        scope: &child,
                        test: context.test || test_only(&module.attrs),
                        ..context
                    },
                );
            }
        }
    }

    fn module(&mut self, module: &syn::ItemMod, context: Context<'_>) {
        let name = module.ident.to_string();
        let custom = path_override(module);
        if let Some((_, items)) = &module.content {
            let inline = context.directory.join(custom.as_deref().unwrap_or(&name));
            self.items(
                items,
                Context {
                    directory: &inline,
                    custom_base: &inline,
                    ..context
                },
            );
        } else if let Some((path, dir)) = self.resolve(
            (context.directory, context.custom_base),
            (&name, custom.as_deref()),
        ) {
            self.file((&path, &dir), (context.scope, context.test));
        }
    }

    fn resolve(
        &self,
        (directory, custom_base): (&Path, &Path),
        (name, custom): (&str, Option<&str>),
    ) -> Option<(PathBuf, PathBuf)> {
        if let Some(custom) = custom {
            let path = self.present(&custom_base.join(custom))?;
            let dir = parent(&path).to_path_buf();
            return Some((path, dir));
        }
        let dir = directory.join(name);
        let path = self
            .present(&directory.join(format!("{name}.rs")))
            .or_else(|| self.present(&dir.join("mod.rs")))?;
        Some((path, dir))
    }

    fn present(&self, path: &Path) -> Option<PathBuf> {
        normalize(path).filter(|path| self.files.contains_key(path.as_path()))
    }
}

fn parent(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new(""))
}

pub(super) fn normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::CurDir => {}
            Component::ParentDir if normalized.pop() => {}
            _ => return None,
        }
    }
    Some(normalized)
}

fn path_override(module: &syn::ItemMod) -> Option<String> {
    module.attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(value) if value.path.is_ident("path") => match &value.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(path),
                ..
            }) => Some(path.value()),
            _ => None,
        },
        _ => None,
    })
}
