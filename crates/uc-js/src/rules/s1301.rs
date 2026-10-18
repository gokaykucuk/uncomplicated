//! "if" statements should be preferred over "switch" when simpler
use crate::helpers::batch_smell1::first_token;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn same_expr(a: &Expression<'_>, b: &Expression<'_>) -> bool {
    match (a, b) {
        (Expression::Identifier(x), Expression::Identifier(y)) => x.name == y.name,
        (Expression::StaticMemberExpression(x), Expression::StaticMemberExpression(y)) => {
            x.property.name == y.property.name && same_expr(&x.object, &y.object)
        }
        _ => false,
    }
}

/// `default` clause that looks like an exhaustiveness check (`const x: never = discriminant`, or
/// a call such as `assertNever(discriminant)` which needs type information to confirm).
fn is_exhaustiveness_check(
    stmts: &[Statement<'_>],
    discriminant: &Expression<'_>,
    is_ts: bool,
) -> bool {
    stmts.iter().any(|s| match s {
        Statement::VariableDeclaration(d) => d.declarations.iter().any(|v| {
            matches!(v.id, BindingPattern::BindingIdentifier(_))
                && v.type_annotation
                    .as_ref()
                    .is_some_and(|t| matches!(t.type_annotation, TSType::TSNeverKeyword(_)))
                && v.init.as_ref().is_some_and(|i| same_expr(i, discriminant))
        }),
        _ if !is_ts => false,
        Statement::ExpressionStatement(e) => call_with(&e.expression, discriminant),
        Statement::ReturnStatement(r) => r
            .argument
            .as_ref()
            .is_some_and(|a| call_with(a, discriminant)),
        Statement::ThrowStatement(t) => call_with(&t.argument, discriminant),
        _ => false,
    })
}

fn call_with(e: &Expression<'_>, discriminant: &Expression<'_>) -> bool {
    matches!(e, Expression::CallExpression(c) if c.arguments.iter().any(|a| a.as_expression().is_some_and(|a| same_expr(a, discriminant))))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchStatement(s) = node.kind() else {
            return;
        };
        let default = s.cases.iter().find(|c| c.test.is_none());
        let n = s.cases.len();
        if !(n < 2 || (n == 2 && default.is_some())) {
            return;
        }
        if let Some(d) = default
            && is_exhaustiveness_check(&d.consequent, &s.discriminant, ctx.is_ts())
        {
            return;
        }
        ctx.report(
            first_token(ctx, s.span.start),
            "Replace this \"switch\" statement by \"if\" statements to increase readability.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn small_switch() {
        check(
            "S1301",
            "a.ts",
            r#"
switch (a) { case 1: f(); }             // Noncompliant {{Replace this "switch" statement by "if" statements to increase readability.}}
switch (a) { case 1: f(); default: g(); } // Noncompliant
switch (a) { case 1: case 2: f(); }
switch (a) {}                           // Noncompliant
switch (x.kind) { case 'a': break; default: const n: never = x.kind; }
switch (x) { case 'a': break; default: assertNever(x); }
"#,
        );
    }
}
