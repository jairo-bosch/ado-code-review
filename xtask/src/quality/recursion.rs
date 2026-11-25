use syn::{
    Block, Expr, ExprCall, ExprClosure, ExprMethodCall, Ident, Item,
    visit::{self, Visit},
};

struct Calls<'a> {
    name: &'a Ident,
    recursive: bool,
}

pub(super) fn direct(name: &Ident, block: &Block) -> bool {
    let mut calls = Calls {
        name,
        recursive: false,
    };
    calls.visit_block(block);
    calls.recursive
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_item(&mut self, _: &'ast Item) {}

    fn visit_expr_closure(&mut self, _: &'ast ExprClosure) {}

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(callee) = call.func.as_ref() {
            let mut path = callee.path.segments.iter();
            self.recursive |= match (path.next(), path.next(), path.next()) {
                (Some(item), None, None) => item.ident == *self.name,
                (Some(owner), Some(item), None) => {
                    (owner.ident == "self" || owner.ident == "Self") && item.ident == *self.name
                }
                _ => false,
            };
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        self.recursive |= call.method == *self.name
            && matches!(call.receiver.as_ref(), Expr::Path(path) if path.path.is_ident("self"));
        visit::visit_expr_method_call(self, call);
    }
}
