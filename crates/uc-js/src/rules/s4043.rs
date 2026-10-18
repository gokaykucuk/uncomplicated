//! Array-mutating methods should not be used misleadingly
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn method_name<'a>(callee: &'a Expression<'a>) -> Option<(&'a str, &'a Expression<'a>)> {
    match callee {
        Expression::StaticMemberExpression(m) => Some((m.property.name.as_str(), &m.object)),
        Expression::ComputedMemberExpression(m) => match &m.expression {
            Expression::StringLiteral(s) => Some((s.value.as_str(), &m.object)),
            _ => None,
        },
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !helpers::batch_smell3::has_type_services(ctx) {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some((method, object)) = method_name(&call.callee) else {
            return;
        };
        let suggested = match method {
            "reverse" => "toReversed",
            "sort" => "toSorted",
            _ => return,
        };
        if !matches!(
            object,
            Expression::Identifier(_)
                | Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)
        ) {
            return;
        }
        if type_of(ctx, object) != Ty::Array {
            return;
        }
        // Self assignment: `a = a.reverse()`.
        if let (AstKind::AssignmentExpression(a), Expression::Identifier(obj)) =
            (ctx.parent_kind(node.id()), object)
            && a.operator == AssignmentOperator::Assign
            && matches!(&a.left, AssignmentTarget::AssignmentTargetIdentifier(l) if l.name == obj.name)
        {
            return;
        }
        // Allowed contexts: standalone statement (through `?.` / logical operators) or returned
        // (through array / object / conditional / spread).
        let mut standalone_ok = true;
        let mut returned_ok = true;
        for anc in helpers::batch_smell3::ancestors(ctx, node.id()) {
            match anc.kind() {
                AstKind::ParenthesizedExpression(_) => {}
                AstKind::ExpressionStatement(_) => {
                    if standalone_ok {
                        return;
                    }
                    break;
                }
                AstKind::ReturnStatement(_) => {
                    if returned_ok {
                        return;
                    }
                    break;
                }
                AstKind::ChainExpression(_) | AstKind::LogicalExpression(_) => returned_ok = false,
                AstKind::ArrayExpression(_)
                | AstKind::ObjectExpression(_)
                | AstKind::ConditionalExpression(_)
                | AstKind::SpreadElement(_) => standalone_ok = false,
                k if is_function(&k) => break,
                _ => {
                    standalone_ok = false;
                    returned_ok = false;
                }
            }
        }
        ctx.report(
            call.span,
            format!("Move this array \"{method}\" operation to a separate statement or replace it with \"{suggested}\"."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn misleading_reverse() {
        check(
            "S4043",
            "a.ts",
            r#"
const a = [1, 2, 3];
const b = a.reverse();          // Noncompliant {{Move this array "reverse" operation to a separate statement or replace it with "toReversed".}}
const c = a.sort();             // Noncompliant {{Move this array "sort" operation to a separate statement or replace it with "toSorted".}}
foo(a['sort']());               // Noncompliant
a.reverse();
a.sort((x, y) => x - y);
function f() { return a.reverse(); }
function g() { return [a.sort()]; }
let d: number[] = [];
d = d.reverse();
[1, 2].reverse().map(x => x);
unknown.reverse().map(x => x);
"#,
        );
    }
}
