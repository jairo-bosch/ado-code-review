use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use syn::{Expr, ExprLit, Item, Lit, Meta};

use super::source::{module_path, test_only};

pub(super) fn exclusions(file: &Path, ast: &syn::File, excluded: &mut BTreeSet<PathBuf>) {
    let directory = module_path(file).into_iter().collect::<PathBuf>();
    let custom_base = file.parent().unwrap_or_else(|| Path::new(""));
    collect(
        &ast.items,
        (&directory, custom_base),
        (test_only(&ast.attrs), excluded),
    );
}

fn collect(
    items: &[Item],
    (directory, custom_base): (&Path, &Path),
    (parent_test_only, excluded): (bool, &mut BTreeSet<PathBuf>),
) {
    for item in items {
        let Item::Mod(module) = item else { continue };
        let path = directory.join(module.ident.to_string());
        let test = parent_test_only || test_only(&module.attrs);
        if let Some((_, nested)) = &module.content {
            collect(nested, (&path, &path), (test, excluded));
        } else if test {
            exclude(module, (path, custom_base), excluded);
        }
    }
}

fn exclude(
    module: &syn::ItemMod,
    (path, custom_base): (PathBuf, &Path),
    out: &mut BTreeSet<PathBuf>,
) {
    if let Some(custom) = path_override(module) {
        out.insert(custom_base.join(custom));
    } else {
        out.insert(path.with_extension("rs"));
        out.insert(path);
    }
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
