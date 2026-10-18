//! Traditional for loops should be replaced with for-of loops
use crate::helpers::batch_unicorn::{const_init, number_value};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_ident(e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == name)
}

fn is_one(e: &Expression<'_>) -> bool {
    number_value(e) == Some(1.0)
}

fn update_ok(update: &Expression<'_>, index: &str) -> bool {
    match strip_parens(update) {
        Expression::UpdateExpression(u) => {
            u.operator == UpdateOperator::Increment
                && matches!(&u.argument, SimpleAssignmentTarget::AssignmentTargetIdentifier(id) if id.name == index)
        }
        Expression::AssignmentExpression(a) => {
            if !matches!(&a.left, AssignmentTarget::AssignmentTargetIdentifier(id) if id.name == index)
            {
                return false;
            }
            match a.operator {
                AssignmentOperator::Addition => is_one(&a.right),
                AssignmentOperator::Assign => {
                    matches!(strip_parens(&a.right), Expression::BinaryExpression(b)
                    if b.operator == BinaryOperator::Addition
                        && ((is_ident(&b.left, index) && is_one(&b.right)) || (is_one(&b.left) && is_ident(&b.right, index))))
                }
                _ => false,
            }
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
        // for (let i = 0; ...)
        let Some(ForStatementInit::VariableDeclaration(decl)) = &f.init else {
            return;
        };
        if decl.declarations.len() != 1 {
            return;
        }
        let d = &decl.declarations[0];
        if d.init.as_ref().and_then(number_value) != Some(0.0) {
            return;
        }
        let BindingPattern::BindingIdentifier(index) = &d.id else {
            return;
        };
        let Some(index_sym) = index.symbol_id.get() else {
            return;
        };
        let index_name = index.name.as_str();
        // i < arr.length / arr.length > i
        let Some(Expression::BinaryExpression(test)) = f.test.as_ref().map(strip_parens) else {
            return;
        };
        let (lesser, greater) = match test.operator {
            BinaryOperator::LessThan => (&test.left, &test.right),
            BinaryOperator::GreaterThan => (&test.right, &test.left),
            _ => return,
        };
        if !is_ident(lesser, index_name) {
            return;
        }
        let Expression::StaticMemberExpression(len) = strip_parens(greater) else {
            return;
        };
        if len.property.name != "length" {
            return;
        }
        let Expression::Identifier(array) = &len.object else {
            return;
        };
        let array_name = array.name.as_str();
        // Statically known non-array values (e.g. strings) are skipped.
        if let Some(init) = const_init(ctx, &len.object)
            && !matches!(init, Expression::ArrayExpression(_))
            && matches!(
                init,
                Expression::StringLiteral(_)
                    | Expression::TemplateLiteral(_)
                    | Expression::NumericLiteral(_)
                    | Expression::ObjectExpression(_)
            )
        {
            return;
        }
        if !f.update.as_ref().is_some_and(|u| update_ok(u, index_name)) {
            return;
        }
        let Statement::BlockStatement(body) = &f.body else {
            return;
        };
        let scoping = ctx.scoping();
        let in_body = |span: Span| body.span.contains_inclusive(span);
        // Index not written in the body.
        for r in scoping.get_resolved_references(index_sym) {
            if r.is_write() && in_body(ctx.node(r.node_id()).kind().span()) {
                return;
            }
        }
        // Array references in the body: all `array[index]` reads.
        let ref_ids: Vec<_> = match ctx.symbol_of(array) {
            Some(sym) => scoping.get_resolved_reference_ids(sym).to_vec(),
            None => scoping
                .root_unresolved_references()
                .get(array_name)
                .map(|v| v.iter().copied().collect())
                .unwrap_or_default(),
        };
        let mut count = 0;
        for r in ref_ids {
            let ref_node = ctx.node(scoping.get_reference(r).node_id());
            if !in_body(ref_node.kind().span()) {
                continue;
            }
            count += 1;
            let member = ctx.parent(ref_node.id());
            let prop_ok = match member.kind() {
                AstKind::ComputedMemberExpression(m) => {
                    matches!(&m.expression, Expression::Identifier(id) if id.name == index_name)
                }
                AstKind::StaticMemberExpression(m) => m.property.name == index_name,
                _ => false,
            };
            if !prop_ok {
                return;
            }
            if let AstKind::AssignmentExpression(a) = ctx.parent_kind(member.id())
                && a.left.span() == member.kind().span()
            {
                return;
            }
        }
        if count == 0 {
            return;
        }
        let end = token_before(ctx, f.body.span().start).map_or(f.span.end, |t| t.end);
        ctx.report(
            Span::new(f.span.start, end),
            "Use a `for-of` loop instead of this `for` loop.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn for_loop() {
        check(
            "S7731",
            "a.js",
            r#"
for (let i = 0; i < arr.length; i++) { // Noncompliant {{Use a `for-of` loop instead of this `for` loop.}}
  console.log(arr[i]);
}
for (let i = 0; arr.length > i; i += 1) { // Noncompliant
  const el = arr[i];
  console.log(i, el);
}
for (var j = 0; j < list.length; j = j + 1) { foo(list[j]); } // Noncompliant
for (let i = 0; i < arr.length; i++) { arr[i] = 0; }
for (let i = 0; i < arr.length; i++) { console.log(arr[i + 1]); }
for (let i = 0; i < arr.length; i++) { i++; foo(arr[i]); }
for (let i = 0; i < arr.length; i++) { console.log(i); }
for (let i = 1; i < arr.length; i++) { console.log(arr[i]); }
for (let i = 0; i < arr.length; i += 2) { console.log(arr[i]); }
for (let i = 0; i < arr.length; i++) foo(arr[i]);
for (let i = 0; i < arr.length; i++) { foo(arr[i], arr.length); }
const str = 'abc';
for (let i = 0; i < str.length; i++) { foo(str[i]); }
"#,
        );
    }
}
