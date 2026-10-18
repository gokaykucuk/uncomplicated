//! Use ".some()" instead of ".filter().length" checks or ".find()" for existence testing
use crate::helpers::batch_unicorn::{
    is_boolean_expression, is_control_flow_test, is_method_call, is_negative_one, paren_parent,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_undefined_ident(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == "undefined")
}

fn is_raw_zero(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.raw.as_deref() == Some("0"))
}

/// Unicorn's `isNodeValueNotFunction`.
fn is_not_function(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ArrayExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::ClassExpression(_)
        | Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::TemplateLiteral(_)
        | Expression::UnaryExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::AssignmentExpression(_)
        | Expression::AwaitExpression(_)
        | Expression::NewExpression(_)
        | Expression::TaggedTemplateExpression(_)
        | Expression::ThisExpression(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::CallExpression(c) => is_method_call(c, &["bind"], None, false).is_none(),
        _ => false,
    }
}

impl Rule {
    fn find_used_as_boolean(
        &self,
        ctx: &Ctx<'_, '_>,
        node: &AstNode<'_>,
        call: &CallExpression<'_>,
    ) -> bool {
        // `x.find(f) === undefined` / `!= null`
        let parent = paren_parent(ctx, node.id());
        if let AstKind::BinaryExpression(b) = parent.kind()
            && strip_parens(&b.left).span() == call.span
        {
            let undef = matches!(
                b.operator,
                BinaryOperator::Equality
                    | BinaryOperator::Inequality
                    | BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
            ) && is_undefined_ident(&b.right);
            let null = matches!(
                b.operator,
                BinaryOperator::Equality | BinaryOperator::Inequality
            ) && is_null(&b.right);
            if undef || null {
                return true;
            }
        }
        if is_boolean_expression(ctx, node.id()) || is_control_flow_test(ctx, node.id()) {
            return true;
        }
        // `const found = x.find(f); if (found) ...`
        let AstKind::VariableDeclarator(d) = parent.kind() else {
            return false;
        };
        if d.type_annotation.is_some() {
            return false;
        }
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return false;
        };
        let decl = ctx.parent(parent.id());
        match decl.kind() {
            AstKind::VariableDeclaration(v) if v.kind == VariableDeclarationKind::Const => {}
            _ => return false,
        }
        if matches!(ctx.parent_kind(decl.id()), AstKind::ExportDeclaration(_)) {
            return false;
        }
        let Some(sym) = id.symbol_id.get() else {
            return false;
        };
        let mut count = 0;
        for r in ctx.scoping().get_resolved_references(sym) {
            count += 1;
            if !r.is_read()
                || !(is_boolean_expression(ctx, r.node_id())
                    || is_control_flow_test(ctx, r.node_id()))
            {
                return false;
            }
        }
        count > 0
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                let Some(m) = is_method_call(call, &["find", "findLast"], None, false) else {
                    return;
                };
                let n = call.arguments.len();
                if !(1..=2).contains(&n)
                    || call
                        .arguments
                        .iter()
                        .any(|a| matches!(a, Argument::SpreadElement(_)))
                    || call.type_arguments.is_some()
                {
                    return;
                }
                if self.find_used_as_boolean(ctx, node, call) {
                    ctx.report(
                        m.property.span,
                        format!("Prefer `.some(…)` over `.{}(…)`.", m.property.name),
                    );
                }
            }
            AstKind::BinaryExpression(b) => {
                let left = strip_parens(&b.left);
                // x.findIndex(f) !== -1
                if let Expression::CallExpression(c) = left
                    && let Some(m) =
                        is_method_call(c, &["findIndex", "findLastIndex"], Some(1), false)
                {
                    use BinaryOperator::*;
                    let ok = (matches!(
                        b.operator,
                        StrictInequality | Inequality | GreaterThan | StrictEquality | Equality
                    ) && is_negative_one(&b.right))
                        || (matches!(b.operator, GreaterEqualThan | LessThan)
                            && matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value == 0.0));
                    if ok {
                        ctx.report(
                            m.property.span,
                            format!("Prefer `.some(…)` over `.{}(…)`.", m.property.name),
                        );
                    }
                    return;
                }
                // x.filter(f).length > 0
                if !matches!(
                    b.operator,
                    BinaryOperator::GreaterThan | BinaryOperator::StrictInequality
                ) || !is_raw_zero(&b.right)
                {
                    return;
                }
                let Expression::StaticMemberExpression(len) = left else {
                    return;
                };
                if len.property.name != "length" || len.optional {
                    return;
                }
                let Expression::CallExpression(filter) = strip_parens(&len.object) else {
                    return;
                };
                let Some(fm) = is_method_call(filter, &["filter"], None, false) else {
                    return;
                };
                if matches!(&fm.object, Expression::Identifier(o) if o.name.starts_with('$')) {
                    return;
                }
                let Some(first) = filter.arguments.first().and_then(|a| a.as_expression()) else {
                    return;
                };
                if is_not_function(first) {
                    return;
                }
                ctx.report(
                    fm.property.span,
                    "Prefer `.some(…)` over non-zero length check from `.filter(…)`.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn array_some() {
        check(
            "S7754",
            "a.js",
            r#"
if (array.find(x => x > 1)) {} // Noncompliant {{Prefer `.some(…)` over `.find(…)`.}}
const a = !!array.findLast(x => x); // Noncompliant {{Prefer `.some(…)` over `.findLast(…)`.}}
const b = array.find(f) !== undefined; // Noncompliant
const c = array.find(f) != null; // Noncompliant
const d = array.findIndex(f) !== -1; // Noncompliant {{Prefer `.some(…)` over `.findIndex(…)`.}}
const e = array.findLastIndex(f) < 0; // Noncompliant
const g = array.filter(x => x).length > 0; // Noncompliant {{Prefer `.some(…)` over non-zero length check from `.filter(…)`.}}
const found = array.find(f); // Noncompliant
if (found) {}
while (foo && array.find(f)) {} // Noncompliant
const h = array.find(f);
const i = array.find(f) === null;
const j = array.findIndex(f) === 0;
const k = $el.filter(f).length > 0;
const l = array.filter('x').length > 0;
const used = array.find(f);
console.log(used.name);
"#,
        );
    }
}
