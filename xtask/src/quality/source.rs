use std::{ffi::OsStr, path::Path};

use syn::{Attribute, Meta, Token, punctuated::Punctuated};

pub(super) fn test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg") && attr.parse_args::<Meta>().is_ok_and(requires_test))
    })
}

fn requires_test(meta: Meta) -> bool {
    if meta.path().is_ident("test") {
        return true;
    }
    let Meta::List(list) = meta else { return false };
    let Ok(items) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return false;
    };
    if list.path.is_ident("all") {
        items.into_iter().any(requires_test)
    } else {
        list.path.is_ident("any") && !items.is_empty() && items.into_iter().all(requires_test)
    }
}

pub(super) fn production(path: &Path) -> bool {
    path.file_name() != Some(OsStr::new("tests.rs"))
}

pub(super) fn module_path(path: &Path) -> Vec<String> {
    let mut parts = path
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if let Some(file) = parts.pop() {
        let module = file.trim_end_matches(".rs");
        if !matches!(module, "lib" | "main" | "mod") {
            parts.push(module.to_owned());
        }
    }
    parts
}
