//! Loop counters should not be assigned within the loop body
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn collect_counters<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<&'b IdentifierReference<'a>>) {
    match e {
        Expression::AssignmentExpression(a) => {
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left {
                out.push(id);
            }
        }
        Expression::UpdateExpression(u) => {
            if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &u.argument {
                out.push(id);
            }
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                collect_counters(e, out);
            }
        }
        _ => {}
    }
}

fn is_splice_with(call: &CallExpression<'_>, name: &str) -> bool {
    matches!(&call.callee, Expression::StaticMemberExpression(m) if m.property.name == "splice")
        && matches!(call.arguments.first(), Some(Argument::Identifier(id)) if id.name == name)
}

/// `arr.splice(i, 1)` / `x = arr.splice(i, 1)` / `const x = arr.splice(i, 1)` directly in a block.
fn block_has_splice(stmts: &[Statement<'_>], name: &str) -> bool {
    stmts.iter().any(|s| match s {
        Statement::ExpressionStatement(e) => match &e.expression {
            Expression::CallExpression(c) => is_splice_with(c, name),
            Expression::AssignmentExpression(a) => {
                matches!(&a.right, Expression::CallExpression(c) if is_splice_with(c, name))
            }
            _ => false,
        },
        Statement::VariableDeclaration(d) => d.declarations.iter().any(
            |d| matches!(&d.init, Some(Expression::CallExpression(c)) if is_splice_with(c, name)),
        ),
        _ => false,
    })
}

/// Increments (`i++`, `i += 2`) and `i = ...` compensating a `splice(i, ...)` are intentional.
fn is_intentional_skip(
    ctx: &Ctx<'_, '_>,
    ref_node: NodeId,
    ref_span: Span,
    body: Span,
    name: &str,
) -> bool {
    for a in ctx.nodes().ancestors(ref_node) {
        if a.kind().span() == body {
            break;
        }
        if let AstKind::ForStatement(f) = a.kind()
            && f.update
                .as_ref()
                .is_some_and(|u| u.span().contains_inclusive(ref_span))
        {
            return false;
        }
    }
    match ctx.parent_kind(ref_node) {
        AstKind::UpdateExpression(_) => true,
        AstKind::AssignmentExpression(a) if a.operator != AssignmentOperator::Assign => true,
        AstKind::AssignmentExpression(_) => {
            ctx.nodes()
                .ancestors(ref_node)
                .find_map(|a| match a.kind() {
                    AstKind::BlockStatement(b) => Some(block_has_splice(&b.body, name)),
                    AstKind::FunctionBody(b) => Some(block_has_splice(&b.statements, name)),
                    _ => None,
                })
                == Some(true)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ForStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ForStatement(f) = node.kind() else {
            return;
        };
        let (Some(update), Statement::BlockStatement(body)) = (&f.update, &f.body) else {
            return;
        };
        let mut counters = Vec::new();
        collect_counters(update, &mut counters);
        for counter in counters {
            let Some(sym) = ctx.symbol_of(counter) else {
                continue;
            };
            for r in ctx.scoping().get_resolved_references(sym) {
                if !r.is_write() {
                    continue;
                }
                let span = ctx.node(r.node_id()).span();
                if span.start <= body.span.start || span.end >= body.span.end {
                    continue;
                }
                if is_intentional_skip(ctx, r.node_id(), span, body.span, &counter.name) {
                    continue;
                }
                ctx.report_with(
                    span,
                    format!("Remove this assignment of \"{}\".", counter.name),
                    [(counter.span, Some("Counter variable update".to_string()))],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn updated_counter() {
        check(
            "S2310",
            "a.js",
            r#"
for (var i = 0; i < 10; i++) {
  i = 5; // Noncompliant {{Remove this assignment of "i".}}
  i++;
  i += 2;
}
for (var j = 0; j < arr.length; j++) {
  if (arr[j] === x) {
    arr.splice(j, 1);
    j = j - 1;
  }
}
for (var k = 0; k < 10; k++) {
  for (; k < 5; k++) {} // Noncompliant
}
for (var m = 0; m < 10; m++) foo(m = 1);
for (const x of xs) { x2 = 1; }
for (var a = 0, b = 0; a < 10; a++, b++) {
  [b] = [1]; // Noncompliant
}
"#,
        );
    }
}
