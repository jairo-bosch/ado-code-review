use std::{fmt::Write as _, path::Path, sync::LazyLock};

use syn::visit::Visit;

use super::{
    audit, check::Methods, complexity::Complexity, config::Quality, counts::Counts, recursion, run,
};

const MANIFEST: &str = include_str!("../../../Cargo.toml");
static QUALITY: LazyLock<Quality> = LazyLock::new(|| Quality::parse(MANIFEST).unwrap());

fn inspect(source: &str, quality: &Quality) -> Vec<String> {
    let mut methods = Methods::new();
    let findings = audit::file(
        &(
            "src/fixture.rs".into(),
            source.lines().count(),
            syn::parse_file(source).unwrap(),
        ),
        quality,
        (vec!["fixture".to_owned()], &mut methods),
    );
    findings
        .into_iter()
        .chain(audit::public_methods(methods, quality))
        .filter(|finding| finding.applies(quality))
        .map(|finding| finding.to_string())
        .collect()
}

#[test]
fn ignores_tests_attributes_constants_and_discriminants() {
    let source = r"
        #[cfg(all(feature = my_feature, test))] fn ignored() { let x = 33; }
        #[cfg(any(test, feature = my_feature))] fn retained() { let a = 34; }
        #[test] fn ignored_too() { let y = 33; }
        #[tag(7)] enum Value { X = 73, Y = 77 }
        const X: i32 = 12;
        static Y: u32 = 11;
        struct Wrap;
        impl Wrap { const Z: i32 = 9; }
        trait WithDefault { const C: i32 = 8; }
        fn active() { let x = [-1, 0, 0x1, 1]; let y = [0.0, 1.0, -0.0, -1.0]; }
        fn macro_call() { show!(7, [1, 1]); }
    ";
    let findings = inspect(source, &QUALITY);
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().all(|item| item.contains("magic-number")));
}

#[test]
fn aggregates_impl_blocks_and_applies_exact_exceptions() {
    let manifest = format!(
        "{MANIFEST}\n[[workspace.metadata.quality.exceptions]]\n\
        path = 'src/fixture.rs'\nrule = 'fields'\nreason = 'Test exception'"
    );
    let quality = Quality::parse(&manifest).unwrap();
    let mut source = "struct Sample(u8, u8, u8, u8, u8, u8);\n".to_owned();
    for method in 0..11 {
        writeln!(source, "impl Sample {{ pub(crate) fn m{method}() {{}} }}").unwrap();
    }
    let findings = inspect(&source, &quality);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].contains("public-methods"));
    assert!(Quality::parse(&manifest.replace("Test exception", " ")).is_err());
    assert!(Quality::parse(&manifest.replace("'fields'", "'bad-rule'")).is_err());
    assert!(Quality::parse(&manifest.replace("src/fixture.rs", "../fixture.rs")).is_err());
    assert!(Quality::parse("[").is_err());
}

#[test]
fn checks_physical_size_and_operand_limit() {
    let source = format!(
        "{}impl Sample {{\n{}}}\n",
        "\n".repeat(110),
        "\n".repeat(201)
    );
    let findings = inspect(&source, &QUALITY);
    assert!(findings.iter().any(|item| item.contains("file-lines")));
    assert!(findings.iter().any(|item| item.contains("impl-lines")));
    assert!(
        inspect("fn f() { if a && (b || c) && d {} }", &QUALITY)
            .iter()
            .any(|item| item.contains("boolean-operands"))
    );
    assert_eq!(
        inspect("fn f() { if a && (b || c) {} }", &QUALITY),
        [] as [String; 0]
    );
}

#[test]
fn measures_nested_decisions_and_boolean_sequences() {
    let block = syn::parse_str(
        r"{
        if a && (b || c) && d {
            for x in xs {
                match x { Some(y) if y => continue, _ => { call()?; } }
            }
        } else if b { loop { break; } } else { while c {} }
    }",
    )
    .unwrap();
    let mut score = Complexity::default();
    score.visit_block(&block);
    assert_eq!(score.cyclomatic + 1, 11);
    assert_eq!(score.cognitive, 19);
    assert_eq!(score.branches, 9);
    assert_eq!(score.boolean_operands, 4);
}

#[test]
fn counts_bindings_and_exit_points_without_nested_functions() {
    let function: syn::ItemFn = syn::parse_str(
        r"fn example(arg: T) -> T {
        let (first, second) = arg;
        for item in first { if let Some(value) = item { return value; } }
        while let Some(next) = second.pop() {}
        match second { Some(ignored) => {}, _ => {} }
        let nested = || { let another = (); };
        #[cfg(test)] let test_only = ();
        second
    }",
    )
    .unwrap();
    let mut counts = Counts::default();
    counts.visit_signature(&function.sig);
    counts.block(&function.block);
    assert_eq!(counts.locals, 8);
    assert_eq!(counts.exits, 2);
    assert_eq!(counts.statements, 8);

    let tail_return = syn::parse_str("{ return value }").unwrap();
    let mut exits = Counts::default();
    exits.block(&tail_return);
    assert_eq!(exits.exits, 1);
}

#[test]
fn limits_locals_bound_in_match_patterns() {
    let findings = inspect(
        "fn example() {
            match value {
                #[cfg(test)] Some(test_only) => {},
                Some((a, b, c, d, e, f, g, h, i, j, k)) if a => {},
                _ => {},
            }
        }
        fn alternatives() {
            match value {
                Ok((a, b, c, d, e, f)) | Err((a, b, c, d, e, f)) => {},
            }
        }",
        &QUALITY,
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("locals"), "{findings:?}");
    assert!(findings[0].contains("11"), "{findings:?}");
}

#[test]
fn aggregates_methods_through_multiple_super_qualifiers() {
    let source = "
        struct Root;
        impl Root {
            pub fn a() {} pub fn b() {} pub fn c() {}
            pub fn d() {} pub fn e() {} pub fn f() {}
        }
        mod outer {
            mod inner {
                impl super::super::Root {
                    pub fn g() {} pub fn h() {} pub fn i() {}
                    pub fn j() {} pub fn k() {}
                }
            }
        }
    ";
    let findings = inspect(source, &QUALITY);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("public-methods"));
}

#[test]
fn reports_source_and_tool_settings_at_the_workspace_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut out = Vec::new();
    run(Some("coverage"), root, &mut out).unwrap();
    assert_eq!(out, format!("{}\n", QUALITY.tools.coverage).as_bytes());
    out.clear();
    run(Some("duplication"), root, &mut out).unwrap();
    assert_eq!(
        out,
        format!(
            "{} {}\n",
            QUALITY.tools.duplicate_lines, QUALITY.tools.duplicate_threshold
        )
        .as_bytes()
    );
    assert!(run(None, root, &mut out).is_err());
    assert!(run(None, &root.join("nonexistent"), &mut out).is_err());
}

#[test]
fn analyzes_impl_trait_and_closure_bodies_independently() {
    let source = r"
        #![allow(dead_code)]
        mod root {
            type Array = [u8; 12];
            enum Value { X([u8; 13]), #[cfg(test)] Y([u8; 14]) }
            struct Case(#[cfg(test)] [u8; 15]);
            trait Calculate { fn eval(&self) { let Some(x) = next() else { return }; call!(16); } }
            impl Calculate for Case {
                fn eval(&self) {
                    let calc = |a| { if a { 17 } else { 18 } };
                    let delegate = |a| return calc(a);
                    let double = |a| a + 19;
                    match value { #[cfg(test)] _ => 20, _ => 21 };
                    #[cfg(test)] let unused = 22;
                }
            }
        }
    ";
    let findings = inspect(source, &QUALITY);
    assert_eq!(findings.len(), 7, "{findings:?}");
    assert!(
        findings
            .iter()
            .all(|finding| finding.contains("magic-number"))
    );
}

#[test]
fn preserves_separate_boolean_sequences_and_labeled_jumps() {
    let block = syn::parse_str(
        r"{
        'outer: loop {
            if a | b { continue 'outer; }
            if a && !(b && c) { break 'outer; }
        }

    }",
    )
    .unwrap();
    let mut score = Complexity::default();
    score.visit_block(&block);
    assert_eq!(score.cyclomatic + 1, 6);
    assert_eq!(score.cognitive, 9);
    assert_eq!(score.boolean_operands, 2);
}

#[test]
fn recognizes_direct_recursion_but_not_calls_on_another_receiver() {
    let name = syn::parse_str("recur").unwrap();
    for body in [
        "{ recur() }",
        "{ self.recur() }",
        "{ Self::recur() }",
        "{ self::recur() }",
    ] {
        assert!(
            recursion::direct(&name, &syn::parse_str(body).unwrap()),
            "{body}"
        );
    }
    let calls =
        syn::parse_str("{ other.recur(); let callback = || recur(); fn recur() {} }").unwrap();
    assert!(!recursion::direct(&name, &calls));
}
