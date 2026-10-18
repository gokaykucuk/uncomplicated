//! Braces and parentheses should be used consistently with arrow functions
use crate::prelude::*;

pub struct Rule {
    require_parens: bool,
    require_braces: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            require_parens: p.bool("requireParameterParentheses", false),
            require_braces: p.bool("requireBodyBraces", false),
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ArrowFunctionExpression(arrow) = node.kind() else {
            return;
        };
        self.check_params(arrow, ctx);
        self.check_body(arrow, ctx);
    }
}

impl Rule {
    fn check_params(&self, arrow: &ArrowFunctionExpression<'_>, ctx: &Ctx<'_, '_>) {
        let params = &arrow.params;
        if params.items.len() + usize::from(params.rest.is_some()) != 1 {
            return;
        }
        let param_span = match params.items.first() {
            Some(p) => p.span,
            None => match &params.rest {
                Some(r) => r.span,
                None => return,
            },
        };
        let has_parens = token_after(ctx, param_span.end).is_some_and(|t| ctx.text(t) == ")");
        if self.require_parens && !has_parens {
            ctx.report(
                param_span,
                "Add parentheses around the parameter of this arrow function.",
            );
        } else if !self.require_parens && has_parens && arrow.type_parameters.is_none() {
            let Some(param) = params.items.first() else {
                return;
            };
            let simple = matches!(param.pattern, BindingPattern::BindingIdentifier(_))
                && param.initializer.is_none()
                && param.type_annotation.is_none()
                && !param.optional
                && arrow.return_type.is_none();
            if simple && !ctx.has_comments_in(Span::new(arrow.span.start, arrow.body.span().start))
            {
                ctx.report(
                    param_span,
                    "Remove parentheses around the parameter of this arrow function.",
                );
            }
        }
    }

    fn check_body(&self, arrow: &ArrowFunctionExpression<'_>, ctx: &Ctx<'_, '_>) {
        match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                if self.require_braces || !body.directives.is_empty() || body.statements.len() != 1
                {
                    return;
                }
                let Statement::ReturnStatement(ret) = &body.statements[0] else {
                    return;
                };
                let Some(arg) = &ret.argument else { return };
                let arg = strip_parens(arg);
                if matches!(arg, Expression::ObjectExpression(_)) {
                    return;
                }
                let s = arg.span();
                if ctx.line(s.start) == ctx.line(s.end) {
                    ctx.report(
                        body.span,
                        "Remove curly braces and \"return\" from this arrow function body.",
                    );
                }
            }
            body => {
                if self.require_braces {
                    ctx.report(
                        body.span(),
                        "Add curly braces and \"return\" to this arrow function body.",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn defaults() {
        check(
            "S3524",
            "a.ts",
            r#"
const a = (x) => x;          // Noncompliant {{Remove parentheses around the parameter of this arrow function.}}
const b = x => { return x; }; // Noncompliant {{Remove curly braces and "return" from this arrow function body.}}
const c = x => x;
const d = (x, y) => x + y;
const e = (x: number) => x;
const f = (x = 1) => x;
const g = ({ a }) => a;
const h = <T>(x) => x;
const i = (x): number => x;
const j = x => { return { a: x }; };
const k = x => { foo(); return x; };
const l = x => {
  return foo(
    x);
};
const m = (/* c */ x) => x;
const n = (...xs) => xs;
const o = async (x) => x;    // Noncompliant
foo((x) => x);               // Noncompliant
"#,
        );
    }

    #[test]
    fn required() {
        check_with(
            "S3524",
            "a.js",
            serde_json::json!({"requireParameterParentheses": true, "requireBodyBraces": true}),
            r#"
const a = x => { return x; };  // Noncompliant {{Add parentheses around the parameter of this arrow function.}}
const b = (x) => x;            // Noncompliant {{Add curly braces and "return" to this arrow function body.}}
const c = (x) => { return x; };
foo(x => { return x; });       // Noncompliant
"#,
        );
    }
}
