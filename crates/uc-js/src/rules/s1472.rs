//! Function call arguments should not start on new lines
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::TaggedTemplateExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                if call.arguments.len() != 1
                    || matches!(strip_parens(&call.callee), Expression::CallExpression(_))
                {
                    return;
                }
                let callee_end = match &call.type_arguments {
                    Some(t) => t.span.end,
                    None => call.callee.span().end,
                };
                let arg_start = call.arguments[0].span().start;
                let between = ctx.tokens_in(Span::new(callee_end, arg_start));
                // Last `)` between the callee and the argument (closing parens around the callee),
                // otherwise the callee's last token.
                let callee_last_line = match between.iter().rev().find(|t| t.kind() == Kind::RParen)
                {
                    Some(t) => ctx.line(t.end()),
                    None => match token_before(ctx, callee_end) {
                        Some(t) => ctx.line(t.end),
                        None => return,
                    },
                };
                let Some(open) = between.iter().find(|t| t.kind() != Kind::RParen) else {
                    return;
                };
                let start_line = ctx.line(open.start());
                if callee_last_line == start_line {
                    return;
                }
                let msg = format!("Make those call arguments start on line {callee_last_line}.");
                if ctx.line(call.span.end) == start_line {
                    ctx.report(Span::new(open.start(), call.span.end), msg);
                } else {
                    ctx.report(Span::new(open.start(), open.start()), msg);
                }
            }
            AstKind::TaggedTemplateExpression(t) => {
                let Some(before) = token_before(ctx, t.quasi.span.start) else {
                    return;
                };
                let before_line = ctx.line(before.start);
                if ctx.line(before.end) != ctx.line(t.quasi.span.start) {
                    ctx.report(
                        Span::new(t.quasi.span.start, t.quasi.span.start + 1),
                        format!("Make this template literal start on line {before_line}."),
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn call_lines() {
        check(
            "S1472",
            "a.js",
            r#"
var a = b
(1 + 2).toString();   // Noncompliant {{Make those call arguments start on line 2.}}
foo(1);
foo(
  1);
var c = (d)
(e);                  // Noncompliant
var s = tag
`hello`;              // Noncompliant {{Make this template literal start on line 9.}}
var t = tag`ok`;
f()
(g);
"#,
        );
    }
}
