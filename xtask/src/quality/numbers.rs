use proc_macro2::{Delimiter, Group, Literal, Span, TokenStream, TokenTree};
use syn::{Expr, Lit};

pub(super) fn allowed(literal: &Lit) -> bool {
    match literal {
        Lit::Int(value) => matches!(value.base10_parse::<u128>(), Ok(0 | 1)),
        Lit::Float(value) => matches!(value.base10_parse::<f64>(), Ok(0.0 | 1.0)),
        _ => true,
    }
}

pub(super) fn unwrapped(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Paren(paren) => &paren.expr,
            Expr::Group(group) => &group.expr,
            _ => return expr,
        };
    }
}

pub(super) fn forbidden_negative(literal: &Lit) -> bool {
    matches!(literal, Lit::Float(value) if !matches!(value.base10_parse::<f64>(), Ok(0.0)))
}

fn forbidden_token(raw: &Literal, negative: bool) -> bool {
    syn::parse_str::<Lit>(&raw.to_string())
        .is_ok_and(|value| !allowed(&value) || (negative && forbidden_negative(&value)))
}

fn parenthesized_literal(mut group: Group) -> Option<Literal> {
    while group.delimiter() == Delimiter::Parenthesis {
        let mut inner = group.stream().into_iter();
        let token = inner.next()?;
        if inner.next().is_some() {
            return None;
        }
        match token {
            TokenTree::Literal(raw) => return Some(raw),
            TokenTree::Group(nested) => group = nested,
            _ => return None,
        }
    }
    None
}

fn group_tokens(group: &Group, negative: bool, warn: &mut impl FnMut(Span)) {
    if negative && let Some(raw) = parenthesized_literal(group.clone()) {
        if forbidden_token(&raw, true) {
            warn(raw.span());
        }
    } else {
        tokens(group.stream(), warn);
    }
}

pub(super) fn tokens(stream: TokenStream, warn: &mut impl FnMut(Span)) {
    let mut expects_operand = true;
    let mut negative = false;
    for token in stream {
        if let TokenTree::Punct(punct) = token {
            let symbol = punct.as_char();
            negative = expects_operand && symbol == '-';
            expects_operand = symbol != '?';
            continue;
        }
        match token {
            TokenTree::Group(group) => group_tokens(&group, negative, warn),
            TokenTree::Literal(raw) if forbidden_token(&raw, negative) => warn(raw.span()),
            _ => {}
        }
        expects_operand = false;
        negative = false;
    }
}
