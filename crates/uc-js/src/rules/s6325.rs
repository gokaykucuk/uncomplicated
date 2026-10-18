//! Regular expression literals should be used when possible
//!
//! ESLint `prefer-regex-literals` with default options.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Global `RegExp`, possibly accessed through a global object (`globalThis.RegExp`, ...).
fn is_global_regexp(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    match strip_parens(callee) {
        Expression::Identifier(id) => ctx.is_global(id, "RegExp"),
        Expression::StaticMemberExpression(m) => {
            m.property.name == "RegExp"
                && matches!(strip_parens(&m.object), Expression::Identifier(o)
                    if ["globalThis", "window", "self", "global"].iter().any(|g| ctx.is_global(o, g)))
        }
        _ => false,
    }
}

fn is_static_string(ctx: &Ctx<'_, '_>, a: &Argument<'_>) -> bool {
    let Some(e) = a.as_expression() else {
        return false;
    };
    match e {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::TaggedTemplateExpression(t) => {
            t.quasi.expressions.is_empty()
                && matches!(&t.tag, Expression::StaticMemberExpression(m)
                    if m.property.name == "raw" && matches!(&m.object, Expression::Identifier(s) if ctx.is_global(s, "String")))
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (span, callee, args) = match node.kind() {
            AstKind::NewExpression(n) => (n.span, &n.callee, &n.arguments),
            AstKind::CallExpression(c) => (c.span, &c.callee, &c.arguments),
            _ => return,
        };
        if !(1..=2).contains(&args.len()) || !args.iter().all(|a| is_static_string(ctx, a)) {
            return;
        }
        if is_global_regexp(ctx, callee) {
            ctx.report(
                span,
                "Use a regular expression literal instead of the 'RegExp' constructor.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_literals() {
        check(
            "S6325",
            "a.js",
            r#"
new RegExp("abc");                 // Noncompliant {{Use a regular expression literal instead of the 'RegExp' constructor.}}
RegExp('abc', 'u');                // Noncompliant
new RegExp(`abc`);                 // Noncompliant
new RegExp(String.raw`^\d\.$`);    // Noncompliant
new globalThis.RegExp("a");        // Noncompliant
new RegExp(pattern);
new RegExp("abc", flags);
new RegExp(`${prefix}abc`);
new RegExp("a" + "b");
new RegExp(/abc/);
new RegExp("a", "g", "x");
function f(RegExp) { new RegExp("a"); }
"#,
        );
    }
}
