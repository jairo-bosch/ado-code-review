use syn::{
    BinOp, Expr, ExprBinary, ExprClosure, ExprForLoop, ExprIf, ExprLoop, ExprMatch, ExprWhile,
    Item, Local, PatGuard,
    visit::{self, Visit},
};

use super::source::test_only;

#[derive(Default)]
pub(super) struct Complexity {
    pub(super) cyclomatic: usize,
    pub(super) cognitive: usize,
    pub(super) branches: usize,
    pub(super) boolean_operands: usize,
    depth: usize,
}

impl Complexity {
    const fn branch(&mut self, amount: usize) {
        self.branches += amount;
        self.cyclomatic += amount;
    }

    fn nested(&mut self, visit: impl FnOnce(&mut Self)) {
        self.depth += 1;
        visit(self);
        self.depth -= 1;
    }

    fn decision(&mut self, visit: impl FnOnce(&mut Self)) {
        self.branch(1);
        self.cognitive += 1 + self.depth;
        self.nested(visit);
    }

    fn conditional(&mut self, node: &ExprIf, else_if: bool) {
        self.branch(1);
        self.cognitive += 1 + if else_if { 0 } else { self.depth };
        self.visit_expr(&node.cond);
        self.nested(|this| this.visit_block(&node.then_branch));
        if let Some((_, other)) = &node.else_branch {
            if let Expr::If(next) = other.as_ref() {
                self.conditional(next, true);
            } else {
                self.branches += 1;
                self.cognitive += 1;
                self.nested(|this| this.visit_expr(other));
            }
        }
    }

    fn boolean(&mut self, expression: &Expr, chain: &mut Chain) {
        match expression {
            Expr::Paren(node) => self.boolean(&node.expr, chain),
            Expr::Group(node) => self.boolean(&node.expr, chain),
            Expr::Binary(node) if operator(&node.op).is_some() => {
                self.boolean_parts(node, chain);
            }
            _ => {
                chain.operands += 1;
                self.visit_expr(expression);
            }
        }
    }

    fn boolean_parts(&mut self, node: &ExprBinary, chain: &mut Chain) {
        self.boolean(&node.left, chain);
        let current = operator(&node.op);
        self.cyclomatic += 1;
        self.cognitive += usize::from(current != chain.previous);
        chain.previous = current;
        self.boolean(&node.right, chain);
    }
}

#[derive(Default)]
struct Chain {
    operands: usize,
    previous: Option<LogicalOperator>,
}

#[derive(PartialEq, Eq)]
enum LogicalOperator {
    And,
    Or,
}

const fn operator(op: &BinOp) -> Option<LogicalOperator> {
    match op {
        BinOp::And(_) => Some(LogicalOperator::And),
        BinOp::Or(_) => Some(LogicalOperator::Or),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for Complexity {
    fn visit_item(&mut self, _: &'ast Item) {}

    fn visit_expr_closure(&mut self, _: &'ast ExprClosure) {}

    fn visit_local(&mut self, node: &'ast Local) {
        if !test_only(&node.attrs) {
            visit::visit_local(self, node);
            if let Some(init) = &node.init
                && init.diverge.is_some()
            {
                self.branch(1);
                self.cognitive += 1 + self.depth;
            }
        }
    }

    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        self.conditional(node, false);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        self.decision(|this| visit::visit_expr_for_loop(this, node));
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.decision(|this| visit::visit_expr_while(this, node));
    }

    fn visit_expr_loop(&mut self, node: &'ast ExprLoop) {
        self.decision(|this| visit::visit_expr_loop(this, node));
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        let arms = node.arms.iter().filter(|arm| !test_only(&arm.attrs));
        let count = arms.clone().count();
        self.branches += count;
        self.cyclomatic += count.saturating_sub(1);
        self.cognitive += 1 + self.depth;
        self.visit_expr(&node.expr);
        self.nested(|this| {
            for arm in arms {
                this.visit_pat(&arm.pat);
                this.visit_expr(&arm.body);
            }
        });
    }

    fn visit_pat_guard(&mut self, node: &'ast PatGuard) {
        self.branch(1);
        self.cognitive += 1 + self.depth;
        visit::visit_pat_guard(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast ExprBinary) {
        if operator(&node.op).is_none() {
            visit::visit_expr_binary(self, node);
            return;
        }
        let mut chain = Chain::default();
        self.boolean_parts(node, &mut chain);
        self.boolean_operands = self.boolean_operands.max(chain.operands);
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        self.cognitive += usize::from(
            matches!(node, Expr::Break(jump) if jump.label.is_some())
                || matches!(node, Expr::Continue(jump) if jump.label.is_some()),
        );
        visit::visit_expr(self, node);
    }
}
