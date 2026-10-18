//! "if ... else if" constructs should end with "else" clauses
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IfStatement(s) = node.kind() else {
            return;
        };
        if s.alternate.is_some() {
            return;
        }
        let AstKind::IfStatement(parent) = ctx.parent_kind(node.id()) else {
            return;
        };
        if !parent
            .alternate
            .as_ref()
            .is_some_and(|a| a.span() == s.span)
        {
            return;
        }
        let Some(else_kw) = token_before(ctx, s.span.start) else {
            return;
        };
        let if_kw =
            find_token(ctx, s.span, Kind::If).unwrap_or(Span::new(s.span.start, s.span.start + 2));
        ctx.report(
            Span::new(else_kw.start, if_kw.end),
            "Add the missing \"else\" clause.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn else_if() {
        check(
            "S126",
            "a.js",
            r#"
if (a) {}
if (a) {} else {}
if (a) {} else if (b) {} // Noncompliant {{Add the missing "else" clause.}}
if (a) {} else if (b) {} else {}
if (a) {}
else if (b) {}
else if (c) {}           // Noncompliant
"#,
        );
    }
}
