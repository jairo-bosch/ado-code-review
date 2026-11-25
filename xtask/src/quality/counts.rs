use syn::{
    Arm, Block, Expr, ExprClosure, ExprReturn, FnArg, Item, PatIdent, Stmt,
    visit::{self, Visit},
};

use super::source::test_only;

#[derive(Default)]
pub(super) struct Counts {
    pub(super) statements: usize,
    pub(super) locals: usize,
    pub(super) exits: usize,
}

impl Counts {
    pub(super) fn block(&mut self, block: &Block) {
        self.exits += usize::from(matches!(
            block.stmts.last(),
            Some(Stmt::Expr(expression, None)) if !matches!(expression, Expr::Return(_))
        ));
        self.visit_block(block);
    }
}

impl<'ast> Visit<'ast> for Counts {
    fn visit_item(&mut self, _: &'ast Item) {}

    fn visit_expr_closure(&mut self, _: &'ast ExprClosure) {}

    fn visit_stmt(&mut self, node: &'ast Stmt) {
        if matches!(node, Stmt::Local(local) if test_only(&local.attrs)) {
            return;
        }
        self.statements += 1;
        visit::visit_stmt(self, node);
    }

    fn visit_fn_arg(&mut self, node: &'ast FnArg) {
        if matches!(node, FnArg::Receiver(_)) {
            self.locals += 1;
        }
        visit::visit_fn_arg(self, node);
    }

    fn visit_pat_ident(&mut self, node: &'ast PatIdent) {
        self.locals += 1;
        visit::visit_pat_ident(self, node);
    }

    fn visit_arm(&mut self, node: &'ast Arm) {
        if !test_only(&node.attrs) {
            visit::visit_arm(self, node);
        }
    }

    fn visit_expr_return(&mut self, node: &'ast ExprReturn) {
        self.exits += 1;
        visit::visit_expr_return(self, node);
    }
}
