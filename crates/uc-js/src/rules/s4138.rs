//! "for of" should be used with Iterables
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_ident(e: &Expression<'_>, name: &str) -> bool {
    matches!(e, Expression::Identifier(id) if id.name == name)
}

fn is_one(e: &Expression<'_>) -> bool {
    matches!(e, Expression::NumericLiteral(n) if n.value == 1.0)
}

/// `i++`, `++i`, `i += 1`, `i = i + 1`, `i = 1 + i`.
fn is_increment(e: &Expression<'_>, name: &str) -> bool {
    match e {
        Expression::UpdateExpression(u) => {
            u.operator == UpdateOperator::Increment
                && matches!(&u.argument, SimpleAssignmentTarget::AssignmentTargetIdentifier(id) if id.name == name)
        }
        Expression::AssignmentExpression(a) => {
            if !matches!(&a.left, AssignmentTarget::AssignmentTargetIdentifier(id) if id.name == name)
            {
                return false;
            }
            match a.operator {
                AssignmentOperator::Addition => is_one(&a.right),
                AssignmentOperator::Assign => matches!(&a.right, Expression::BinaryExpression(b)
                    if b.operator == BinaryOperator::Addition
                        && ((is_ident(&b.left, name) && is_one(&b.right)) || (is_one(&b.left) && is_ident(&b.right, name)))),
                _ => false,
            }
        }
        _ => false,
    }
}

/// True if the member expression node is written to (assigned, deleted, updated, destructured).
fn is_assignee(ctx: &Ctx<'_, '_>, member: &AstNode<'_>) -> bool {
    let span = member.kind().span();
    let parent = ctx.parent(member.id());
    match parent.kind() {
        AstKind::AssignmentExpression(a) => a.left.span() == span,
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::Delete,
        AstKind::UpdateExpression(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::ObjectAssignmentTarget(_)
        | AstKind::AssignmentTargetRest(_)
        | AstKind::AssignmentTargetWithDefault(_)
        | AstKind::AssignmentTargetPropertyProperty(_)
        | AstKind::ForInStatement(_)
        | AstKind::ForOfStatement(_) => true,
        AstKind::TSAsExpression(_)
        | AstKind::TSNonNullExpression(_)
        | AstKind::TSSatisfiesExpression(_)
        | AstKind::TSTypeAssertion(_)
        | AstKind::ParenthesizedExpression(_) => is_assignee(ctx, parent),
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
        let Some(ForStatementInit::VariableDeclaration(decl)) = &f.init else {
            return;
        };
        if decl.declarations.len() != 1 {
            return;
        }
        let d = &decl.declarations[0];
        let BindingPattern::BindingIdentifier(index) = &d.id else {
            return;
        };
        if !matches!(&d.init, Some(Expression::NumericLiteral(n)) if n.value == 0.0) {
            return;
        }
        let name = index.name.as_str();
        let Some(Expression::BinaryExpression(test)) = &f.test else {
            return;
        };
        if test.operator != BinaryOperator::LessThan || !is_ident(&test.left, name) {
            return;
        }
        let Expression::StaticMemberExpression(len) = &test.right else {
            return;
        };
        if len.property.name != "length" {
            return;
        }
        let array_text = ctx.text(len.object.span());
        if !f.update.as_ref().is_some_and(|u| is_increment(u, name)) {
            return;
        }
        let Some(sym) = index.symbol_id.get() else {
            return;
        };
        let body_span = f.body.span();
        let scoping = ctx.scoping();
        for &rid in scoping.get_resolved_reference_ids(sym) {
            let ref_node = scoping.get_reference(rid).node_id();
            let ref_span = ctx.node(ref_node).kind().span();
            if !(body_span.start <= ref_span.start && ref_span.end <= body_span.end) {
                continue;
            }
            let member = ctx.parent(ref_node);
            let AstKind::ComputedMemberExpression(m) = member.kind() else {
                return;
            };
            if m.expression.span() != ref_span
                || matches!(m.object, Expression::ThisExpression(_))
                || ctx.text(m.object.span()) != array_text
                || is_assignee(ctx, member)
            {
                return;
            }
        }
        ctx.report(
            f.span,
            "Use a \"for...of\" loop, since the index is only used to read array elements.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_for_of() {
        check(
            "S4138",
            "a.js",
            r#"
for (let i = 0; i < arr.length; i++) {     // Noncompliant {{Use a "for...of" loop, since the index is only used to read array elements.}}
  console.log(arr[i]);
}
for (let i = 0; i < arr.length; i += 1) {  // Noncompliant
  foo(arr[i], arr[i]);
}
for (let i = 0; i < arr.length; i++) {
  console.log(i, arr[i]);
}
for (let i = 0; i < arr.length; i++) {
  arr[i] = 0;
}
for (let i = 0; i < arr.length; i++) {
  other[i]++;
}
for (let i = 1; i < arr.length; i++) {
  console.log(arr[i]);
}
for (let i = 0; i < arr.length; i += 2) {
  console.log(arr[i]);
}
for (let i = 0; i < this.items.length; i = i + 1) { // Noncompliant
  use(this.items[i]);
}
"#,
        );
    }
}
