//! Nested blocks of code should not be left empty
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

pub struct Rule {
    allow_empty_catch: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_empty_catch: p.bool("allowEmptyCatch", true),
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BlockStatement, AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BlockStatement(b) => {
                if !b.body.is_empty() {
                    return;
                }
                if self.allow_empty_catch
                    && matches!(ctx.parent_kind(node.id()), AstKind::CatchClause(_))
                {
                    return;
                }
                if ctx.has_comments_in(b.span) {
                    return;
                }
                if !eslint_disabled(ctx, b.span.start, "no-empty") {
                    ctx.report(b.span, "Empty block statement.");
                }
            }
            AstKind::SwitchStatement(s)
                if s.cases.is_empty() && !eslint_disabled(ctx, s.span.start, "no-empty") =>
            {
                ctx.report(s.span, "Empty switch statement.");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_blocks() {
        check(
            "S108",
            "a.js",
            r#"
if (a) {}          // Noncompliant {{Empty block statement.}}
if (a) { /* ok */ }
while (a) {}       // Noncompliant
try { f(); } catch (e) {}
try { f(); } finally {} // Noncompliant
switch (a) {}      // Noncompliant {{Empty switch statement.}}
function f() {}
const g = () => {};
class A { static {} m() {} }
for (;;) { x(); }
"#,
        );
        check_with(
            "S108",
            "b.js",
            serde_json::json!({"allowEmptyCatch": false}),
            "try { f(); } catch (e) {} // Noncompliant\n",
        );
    }
}
