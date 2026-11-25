use std::{collections::BTreeMap, path::PathBuf};

use proc_macro2::{Span, TokenStream};
use syn::{
    Arm, Attribute, Block, Expr, ExprClosure, ExprUnary, ImplItem, Lit, Local, Macro, Signature,
    Type, Variant, Visibility,
    spanned::Spanned,
    visit::{self, Visit},
};

use super::{
    complexity::Complexity, config::Quality, counts::Counts, findings::Report, numbers, recursion,
    source::test_only,
};

pub(super) type Methods = BTreeMap<Vec<String>, (PathBuf, Span, usize)>;

pub(super) struct Check<'a> {
    pub(super) quality: &'a Quality,
    pub(super) report: Report,
    pub(super) methods: &'a mut Methods,
    pub(super) module: Vec<String>,
}

impl Check<'_> {
    fn function(&mut self, signature: &Signature, block: &Block) {
        let mut counts = Counts::default();
        counts.visit_signature(signature);
        counts.block(block);
        let mut complexity = Complexity::default();
        complexity.visit_block(block);
        complexity.cognitive += usize::from(recursion::direct(&signature.ident, block));
        self.metrics(signature.span(), (&counts, &complexity));
    }

    fn metrics(&mut self, span: Span, (counts, complexity): (&Counts, &Complexity)) {
        let counts_limit = &self.quality.counts;
        let limits = &self.quality.function;
        for (rule, actual, maximum) in [
            ("statements", counts.statements, counts_limit.statements),
            ("locals", counts.locals, counts_limit.locals),
            ("exits", counts.exits, counts_limit.exits),
            ("cyclomatic", complexity.cyclomatic + 1, limits.cyclomatic),
            ("cognitive", complexity.cognitive, limits.cognitive),
            ("branches", complexity.branches, limits.branches),
            (
                "boolean-operands",
                complexity.boolean_operands,
                limits.boolean_operands,
            ),
        ] {
            self.report.limit((span, rule), (actual, maximum));
        }
    }

    fn inherent_methods(&mut self, node: &syn::ItemImpl) {
        let Type::Path(ty) = node.self_ty.as_ref() else {
            return;
        };
        let count = node
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item, ImplItem::Fn(method)
                        if !matches!(method.vis, Visibility::Inherited) && !test_only(&method.attrs)
                )
            })
            .count();
        let mut segments = ty
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .peekable();
        let mut key = self.module.clone();
        resolve_scope(&mut key, &mut segments);
        key.extend(segments);
        self.methods
            .entry(key)
            .or_insert_with(|| (self.report.path.clone(), node.span(), 0))
            .2 += count;
    }

    fn magic(&mut self, span: Span) {
        self.report.add(
            (span, "magic-number"),
            "give the numeric literal a named constant".to_owned(),
        );
    }

    fn token_numbers(&mut self, tokens: TokenStream) {
        numbers::tokens(tokens, &mut |span| self.magic(span));
    }
}

fn resolve_scope(
    key: &mut Vec<String>,
    segments: &mut std::iter::Peekable<impl Iterator<Item = String>>,
) {
    match segments.peek().map(String::as_str) {
        Some("crate") => {
            key.truncate(1);
            segments.next();
        }
        Some("self") => {
            segments.next();
        }
        _ => {
            while segments.next_if(|segment| segment == "super").is_some() {
                if key.len() > 1 {
                    key.pop();
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Check<'_> {
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_item_const(&mut self, _: &'ast syn::ItemConst) {}

    fn visit_item_static(&mut self, _: &'ast syn::ItemStatic) {}

    fn visit_impl_item_const(&mut self, _: &'ast syn::ImplItemConst) {}

    fn visit_trait_item_const(&mut self, _: &'ast syn::TraitItemConst) {}

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !test_only(&node.attrs) {
            self.function(&node.sig, &node.block);
            visit::visit_item_fn(self, node);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if test_only(&node.attrs) {
            return;
        }
        let span = node.span();
        let lines = span.end().line.saturating_sub(span.start().line) + 1;
        self.report
            .limit((span, "impl-lines"), (lines, self.quality.size.impl_lines));
        if node.trait_.is_none() {
            self.inherent_methods(node);
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if !test_only(&node.attrs) {
            let fields = node
                .fields
                .iter()
                .filter(|field| !test_only(&field.attrs))
                .count();
            self.report
                .limit((node.span(), "fields"), (fields, self.quality.size.fields));
            visit::visit_item_struct(self, node);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !test_only(&node.attrs) {
            self.module.push(node.ident.to_string());
            visit::visit_item_mod(self, node);
            self.module.pop();
        }
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if !test_only(&node.attrs) {
            visit::visit_item_enum(self, node);
        }
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if !test_only(&node.attrs) {
            visit::visit_item_trait(self, node);
        }
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        if !test_only(&node.attrs) {
            visit::visit_item_type(self, node);
        }
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if !test_only(&node.attrs) {
            visit::visit_item_macro(self, node);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !test_only(&node.attrs) {
            self.function(&node.sig, &node.block);
            visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if !test_only(&node.attrs) {
            if let Some(block) = &node.default {
                self.function(&node.sig, block);
            }
            visit::visit_trait_item_fn(self, node);
        }
    }

    fn visit_variant(&mut self, node: &'ast Variant) {
        if !test_only(&node.attrs) {
            self.visit_fields(&node.fields);
        }
    }

    fn visit_field(&mut self, node: &'ast syn::Field) {
        if !test_only(&node.attrs) {
            visit::visit_field(self, node);
        }
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if !test_only(&node.attrs) {
            visit::visit_local(self, node);
        }
    }

    fn visit_arm(&mut self, node: &'ast Arm) {
        if !test_only(&node.attrs) {
            visit::visit_arm(self, node);
        }
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        if test_only(&node.attrs) {
            return;
        }
        let mut counts = Counts::default();
        for input in &node.inputs {
            counts.visit_pat(input);
        }
        if let Expr::Block(body) = node.body.as_ref() {
            counts.block(&body.block);
        } else {
            counts.exits += usize::from(!matches!(node.body.as_ref(), Expr::Return(_)));
            counts.visit_expr(&node.body);
        }
        let mut complexity = Complexity::default();
        complexity.visit_expr(&node.body);
        self.metrics(node.span(), (&counts, &complexity));
        visit::visit_expr_closure(self, node);
    }

    fn visit_expr_unary(&mut self, node: &'ast ExprUnary) {
        if let Expr::Lit(literal) = node.expr.as_ref()
            && numbers::allowed(&literal.lit)
            && numbers::forbidden_negative(&node.op, &literal.lit)
        {
            self.magic(node.span());
        }
        visit::visit_expr_unary(self, node);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        if !numbers::allowed(node) {
            self.magic(node.span());
        }
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        self.token_numbers(node.tokens.clone());
    }
}
