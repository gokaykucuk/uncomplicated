//! Boolean expressions should not be gratuitous
use crate::helpers::batch_security1::{enclosing_fn, parents, sec};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn as_ident<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b IdentifierReference<'a>> {
    match strip_parens(e) {
        Expression::Identifier(id) => Some(id),
        _ => None,
    }
}

fn as_not<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    match strip_parens(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            Some(&u.argument)
        }
        _ => None,
    }
}

/// Identifiers known truthy / falsy when `test` holds.
fn collect<'b, 'a>(test: &'b Expression<'a>, out: &mut Vec<(&'b IdentifierReference<'a>, bool)>) {
    let check = |e: &'b Expression<'a>, out: &mut Vec<(&'b IdentifierReference<'a>, bool)>| {
        if let Some(id) = as_ident(e) {
            out.push((id, true));
        } else if let Some(arg) = as_not(e) {
            if let Some(id) = as_ident(arg) {
                out.push((id, false));
            } else if let Some(id) = as_not(arg).and_then(as_ident) {
                out.push((id, true));
            }
        }
    };
    let mut cur = strip_parens(test);
    check(cur, out);
    while let Expression::LogicalExpression(l) = cur {
        if l.operator != LogicalOperator::And {
            break;
        }
        check(&l.right, out);
        cur = strip_parens(&l.left);
    }
    check(cur, out);
}

/// Span of the write location of a declaration that ESLint counts as a write reference.
fn declaration_write(ctx: &Ctx<'_, '_>, sym: SymbolId) -> Option<NodeId> {
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::VariableDeclarator(v) => {
            let in_for = matches!(
                ctx.parent_kind(ctx.parent(decl.id()).id()),
                AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
            );
            (v.init.is_some() || in_for).then_some(decl.id())
        }
        _ => None,
    }
}

fn might_be_written(
    ctx: &Ctx<'_, '_>,
    sym: SymbolId,
    block: Span,
    block_fn: Option<NodeId>,
) -> bool {
    let check = |id: NodeId| {
        let s = ctx.node(id).kind().span();
        (s.start >= block.start && s.end <= block.end) || enclosing_fn(ctx, id) != block_fn
    };
    if declaration_write(ctx, sym).is_some_and(check) {
        return true;
    }
    ctx.scoping()
        .get_resolved_references(sym)
        .any(|r| r.is_write() && check(r.node_id()))
}

fn in_condition_position(ctx: &Ctx<'_, '_>, id: NodeId, span: Span) -> bool {
    let mut child = span;
    let mut parent = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(p) = parent.kind() {
        child = p.span;
        parent = ctx.parent(parent.id());
    }
    match parent.kind() {
        AstKind::LogicalExpression(l) => match l.operator {
            LogicalOperator::And => {
                // `{cond && x}` in JSX renders `x`.
                let rhs_top = l.right.span() == child
                    && !matches!(ctx.parent_kind(parent.id()), AstKind::LogicalExpression(_));
                !(rhs_top
                    && parents(ctx, parent.id())
                        .any(|a| matches!(a.kind(), AstKind::JSXExpressionContainer(_))))
            }
            LogicalOperator::Or => l.left.span() == child,
            LogicalOperator::Coalesce => false,
        },
        AstKind::IfStatement(_) => true,
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::LogicalNot,
        _ => false,
    }
}

impl Rule {
    fn check_block(
        &self,
        ctx: &Ctx<'_, '_>,
        if_id: NodeId,
        block: Span,
        known: &[(&IdentifierReference<'_>, bool)],
    ) {
        let block_fn = enclosing_fn(ctx, if_id);
        for (id, truthy) in known {
            let Some(sym) = ctx.symbol_of(id) else {
                continue;
            };
            if might_be_written(ctx, sym, block, block_fn) {
                continue;
            }
            let value = if *truthy { "truthy" } else { "falsy" };
            for r in ctx.scoping().get_resolved_references(sym) {
                let span = ctx.node(r.node_id()).kind().span();
                if span.start < block.start || span.end > block.end {
                    continue;
                }
                if in_condition_position(ctx, r.node_id(), span) {
                    ctx.report_with(
                        span,
                        format!(
                            "This always evaluates to {value}. Consider refactoring this code."
                        ),
                        [sec(id.span, Some(&format!("Evaluated here to be {value}")))],
                    );
                }
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IfStatement(s) = node.kind() else {
            return;
        };
        if let Expression::BooleanLiteral(b) = &s.test {
            let value = if b.value { "truthy" } else { "falsy" };
            ctx.report(
                b.span,
                format!("This always evaluates to {value}. Consider refactoring this code."),
            );
        }
        if let Statement::BlockStatement(block) = &s.consequent {
            let mut known = Vec::new();
            collect(&s.test, &mut known);
            if !known.is_empty() {
                self.check_block(ctx, node.id(), block.span, &known);
            }
        }
        if let Some(Statement::BlockStatement(block)) = &s.alternate
            && let Some(id) = as_ident(&s.test)
        {
            self.check_block(ctx, node.id(), block.span, &[(id, false)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn gratuitous() {
        check(
            "S2589",
            "a.js",
            r#"
function f(a, b, c) {
  if (a) {
    if (a) {} // Noncompliant {{This always evaluates to truthy. Consider refactoring this code.}}
    foo(a);
  }
  if (a && !b) {
    if (b) {} // Noncompliant {{This always evaluates to falsy. Consider refactoring this code.}}
    x = a || c; // Noncompliant
    y = c || a;
  }
  if (c) {
    c = next();
    if (c) {}
  } else {
    if (!c) {} // Noncompliant {{This always evaluates to falsy. Consider refactoring this code.}}
  }
  if (true) {} // Noncompliant
  if (b) bar(b && 1);
}
function g() {
  let d = compute();
  if (d) {
    if (d) {} // Noncompliant
  }
}
let e = init();
function h() {
  if (e) {
    if (e) {}
  }
}
"#,
        );
    }
}
