//! Dynamic code execution should not use user-controlled data
use crate::helpers::batch_security1::{fqn, is_literal};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const EVAL_LIKE: &[&str] = &[
    "eval",
    "Function",
    "vm.Script",
    "vm.SourceTextModule",
    "vm.runInContext",
    "vm.runInNewContext",
    "vm.runInThisContext",
];
const SAFE_CODE: &str = "Make sure that this dynamic injection or execution of code is safe.";
const SCRIPT_URL: &str = "Make sure that 'javascript:' code is safe as it is a form of eval().";

/// ESTree literal: `Literal` or a template literal without expressions.
fn is_hardcoded(e: &Expression<'_>) -> bool {
    is_literal(e)
        || matches!(strip_parens(e), Expression::TemplateLiteral(t) if t.expressions.is_empty())
}

fn as_concat<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b BinaryExpression<'a>> {
    match strip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_variable_concat(b: &BinaryExpression<'_>) -> bool {
    if !is_hardcoded(&b.right) {
        return true;
    }
    match as_concat(&b.left) {
        Some(l) => is_variable_concat(l),
        None => !is_hardcoded(&b.left),
    }
}

fn leftmost_string<'a>(b: &BinaryExpression<'a>) -> Option<&'a str> {
    if let Some(l) = as_concat(&b.left) {
        return leftmost_string(l);
    }
    match strip_parens(&b.left) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            Some(t.quasis[0].value.raw.as_str())
        }
        _ => None,
    }
}

fn starts_with_js_scheme(s: &str) -> bool {
    s.len() >= 11 && s.as_bytes()[..11].eq_ignore_ascii_case(b"javascript:")
}

impl Rule {
    fn check_call<'a>(&self, ctx: &Ctx<'a, '_>, callee: &Expression<'a>, args: &[Argument<'a>]) {
        if !matches!(
            strip_parens(callee),
            Expression::Identifier(_)
                | Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)
        ) {
            return;
        }
        let Some(name) = callee_name_of(callee) else {
            return;
        };
        if !matches!(
            name,
            "eval"
                | "Function"
                | "Script"
                | "SourceTextModule"
                | "runInContext"
                | "runInNewContext"
                | "runInThisContext"
        ) {
            return;
        }
        let Some(q) = fqn(ctx, callee) else { return };
        if EVAL_LIKE.contains(&q.as_str())
            && args
                .iter()
                .any(|a| a.as_expression().is_none_or(|e| !is_hardcoded(e)))
        {
            ctx.report(callee.span(), SAFE_CODE);
        }
    }
}

fn callee_name_of<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match strip_parens(e) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        Expression::ComputedMemberExpression(m) => match &m.expression {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        },
        _ => None,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let src = ctx.src.as_bytes();
        let has = |n: &[u8]| memchr::memmem::find(src, n).is_some();
        let calls = has(b"eval") || has(b"Function") || has(b"vm") || has(b"runIn");
        let scheme = has(b"cript:") || has(b"CRIPT:");
        if !calls && !scheme {
            return;
        }
        for node in ctx.nodes().iter() {
            let relevant = match node.kind() {
                AstKind::CallExpression(_) | AstKind::NewExpression(_) => calls,
                AstKind::TemplateLiteral(_) | AstKind::BinaryExpression(_) => scheme,
                _ => false,
            };
            if relevant {
                self.run(node, ctx);
            }
        }
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) => self.check_call(ctx, &c.callee, &c.arguments),
            AstKind::NewExpression(n) => self.check_call(ctx, &n.callee, &n.arguments),
            AstKind::TemplateLiteral(t) => {
                if !t.expressions.is_empty()
                    && starts_with_js_scheme(t.quasis[0].value.raw.as_str())
                {
                    ctx.report(t.span, SCRIPT_URL);
                }
            }
            AstKind::BinaryExpression(b) => {
                if b.operator != BinaryOperator::Addition {
                    return;
                }
                let mut parent = ctx.parent(node.id());
                while let AstKind::ParenthesizedExpression(_) = parent.kind() {
                    parent = ctx.parent(parent.id());
                }
                if matches!(parent.kind(), AstKind::BinaryExpression(p) if p.operator == BinaryOperator::Addition)
                {
                    return;
                }
                if is_variable_concat(b) && leftmost_string(b).is_some_and(starts_with_js_scheme) {
                    ctx.report(b.span, SCRIPT_URL);
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
    fn code_eval() {
        check(
            "S1523",
            "a.js",
            r#"
eval(code); // Noncompliant {{Make sure that this dynamic injection or execution of code is safe.}}
eval("1 + 1");
new Function("a", body); // Noncompliant
Function(`return ${x}`); // Noncompliant
new Function("a", "return a");
const vm = require('vm');
vm.runInNewContext(code, sandbox); // Noncompliant
vm.runInNewContext("1", {}); // Noncompliant
const { runInThisContext } = require('node:vm');
runInThisContext(code); // Noncompliant
function f(eval) { eval(x); }
other.eval(x);
location.href = `javascript:${code}`; // Noncompliant {{Make sure that 'javascript:' code is safe as it is a form of eval().}}
location.href = "javascript:" + code; // Noncompliant
location.href = "JavaScript:" + a + "b"; // Noncompliant
location.href = "javascript:void(0)";
location.href = "http:" + code;
"#,
        );
    }
}
