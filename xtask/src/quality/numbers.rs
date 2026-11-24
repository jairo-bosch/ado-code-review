use proc_macro2::{Span, TokenStream, TokenTree};
use syn::{Lit, UnOp};

pub(super) fn allowed(literal: &Lit) -> bool {
    match literal {
        Lit::Int(value) => matches!(value.base10_parse::<u128>(), Ok(0 | 1)),
        Lit::Float(value) => matches!(value.base10_parse::<f64>(), Ok(0.0 | 1.0)),
        _ => true,
    }
}

pub(super) fn forbidden_negative(operator: &UnOp, literal: &Lit) -> bool {
    matches!(operator, UnOp::Neg(_))
        && matches!(literal, Lit::Float(value) if !matches!(value.base10_parse::<f64>(), Ok(0.0)))
}

pub(super) fn tokens(stream: TokenStream, warn: &mut impl FnMut(Span)) {
    for token in stream {
        match token {
            TokenTree::Group(group) => tokens(group.stream(), warn),
            TokenTree::Literal(raw) => {
                if let Ok(value) = syn::parse_str::<Lit>(&raw.to_string())
                    && !allowed(&value)
                {
                    warn(raw.span());
                }
            }
            _ => {}
        }
    }
}
