//! "for" loop increment clauses should modify the loops' counters
use oxc_ast_visit::{Visit, walk};

use crate::helpers::batch_smell2::callee_root_identifier;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Expressions updated by the increment clause.
#[derive(Default)]
struct Updated {
    spans: Vec<Span>,
}

impl Updated {
    fn target(&mut self, t: &AssignmentTarget<'_>) {
        match t {
            AssignmentTarget::ArrayAssignmentTarget(a) => {
                for e in a.elements.iter().flatten() {
                    self.maybe_default(e);
                }
                if let Some(r) = &a.rest {
                    self.target(&r.target);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(o) => {
                for p in &o.properties {
                    match p {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                            self.spans.push(i.binding.span);
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            self.maybe_default(&p.binding)
                        }
                    }
                }
                if let Some(r) = &o.rest {
                    self.target(&r.target);
                }
            }
            t => self.spans.push(t.span()),
        }
    }

    fn maybe_default(&mut self, t: &AssignmentTargetMaybeDefault<'_>) {
        match t {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => self.target(&d.binding),
            t => {
                if let Some(t) = t.as_assignment_target() {
                    self.target(t);
                }
            }
        }
    }
}

impl<'a> Visit<'a> for Updated {
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        self.target(&it.left);
        walk::walk_assignment_expression(self, it);
    }
    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        self.spans.push(it.argument.span());
        walk::walk_update_expression(self, it);
    }
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(root) = callee_root_identifier(it) {
            self.spans.push(root.span);
        }
        walk::walk_call_expression(self, it);
    }
}

/// Expressions tested by the loop condition.
struct Tested {
    stack: Vec<AstType>,
    spans: Vec<Span>,
}

impl Tested {
    /// Parent type, skipping parentheses (absent from ESTree).
    fn parent(&self) -> Option<AstType> {
        let below = self.stack.len().saturating_sub(1);
        self.stack[..below]
            .iter()
            .rev()
            .find(|t| **t != AstType::ParenthesizedExpression)
            .copied()
    }
}

impl<'a> Visit<'a> for Tested {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        self.stack.push(kind.ty());
        match kind {
            AstKind::IdentifierReference(id) => self.spans.push(id.span),
            AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_) => {
                let is_nested = matches!(
                    self.parent(),
                    Some(
                        AstType::StaticMemberExpression
                            | AstType::ComputedMemberExpression
                            | AstType::PrivateFieldExpression
                            | AstType::CallExpression
                    )
                );
                if !is_nested {
                    self.spans.push(kind.span());
                }
            }
            _ => {}
        }
    }
    fn leave_node(&mut self, _: AstKind<'a>) {
        self.stack.pop();
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
        let (Some(test), Some(update)) = (&f.test, &f.update) else {
            return;
        };
        let mut updated = Updated::default();
        updated.visit_expression(update);
        if updated.spans.is_empty() {
            return;
        }
        let mut tested = Tested {
            stack: Vec::new(),
            spans: Vec::new(),
        };
        tested.visit_expression(test);
        let intersects = tested
            .spans
            .iter()
            .any(|t| updated.spans.iter().any(|u| ctx.same_tokens(*t, *u)));
        if intersects {
            return;
        }
        let join = |spans: &[Span]| {
            spans
                .iter()
                .map(|s| ctx.text(*s))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let for_kw = Span::new(f.span.start, f.span.start + 3);
        ctx.report(
            for_kw,
            format!(
                "This loop's stop condition tests \"{}\" but the incrementer updates \"{}\".",
                join(&tested.spans),
                join(&updated.spans)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn misplaced_counter() {
        check(
            "S1994",
            "a.js",
            r#"
for (i = 0; i < 10; j++) {} // Noncompliant {{This loop's stop condition tests "i" but the incrementer updates "j".}}
for (i = 0; i < 10; i++) {}
for (i = 0; i < arr.length; i++) {}
for (var it = list.iterator(); it.hasNext(); it.next()) {}
for (i = 0; obj.count < 10; obj.count++) {}
for (i = 0; check(i); j += 2) {} // Noncompliant {{This loop's stop condition tests "check, i" but the incrementer updates "j".}}
for (i = 0; i < 10;) {}
for (;; i++) {}
for (i = 0, j = 0; i < 10; [i, j] = [j, i]) {}
for (i = 0; (i < 10); (i++)) {}
"#,
        );
    }
}
