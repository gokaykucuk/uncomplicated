//! A "while" loop should be used instead of a "for" loop
use crate::helpers::batch_smell1::first_token;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        if f.init.is_none() && f.update.is_none() && f.test.is_some() {
            ctx.report(
                first_token(ctx, f.span.start),
                "Replace this \"for\" loop with a \"while\" loop.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_while() {
        check(
            "S1264",
            "a.js",
            r#"
for (; a;) {}       // Noncompliant {{Replace this "for" loop with a "while" loop.}}
for (;;) {}
for (i = 0; a;) {}
for (; a; i++) {}
while (a) {}
"#,
        );
    }
}
