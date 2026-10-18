//! Function calls should not pass extra arguments
use crate::helpers::batch_security1::{FnLike, sec, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Function a call's callee denotes: a function declaration, a never-reassigned variable
/// initialized with a function, or an IIFE.
fn called_function<'a>(ctx: &Ctx<'a, '_>, callee: &'a Expression<'a>) -> Option<FnLike<'a, 'a>> {
    match strip_parens(callee) {
        Expression::FunctionExpression(f) => Some(FnLike::Func(f)),
        Expression::ArrowFunctionExpression(a) => Some(FnLike::Arrow(a)),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) if f.is_declaration() => Some(FnLike::Func(f)),
                AstKind::VariableDeclarator(_) => match unique_init(ctx, sym)? {
                    Expression::FunctionExpression(f) => Some(FnLike::Func(f)),
                    Expression::ArrowFunctionExpression(a) => Some(FnLike::Arrow(a)),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// The body mentions `arguments` (conservatively, any token).
fn uses_arguments(ctx: &Ctx<'_, '_>, f: FnLike<'_, '_>) -> bool {
    let Some(body) = f.body_span() else {
        return true;
    };
    ctx.tokens_in(body)
        .iter()
        .any(|t| ctx.text(t.span()) == "arguments")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(f) = called_function(ctx, &call.callee) else {
            return;
        };
        let params = f.params();
        if params.rest.is_some() || call.arguments.len() <= params.items.len() {
            return;
        }
        // Declared with a body (not an overload / ambient declaration).
        if f.body_span().is_none() {
            return;
        }
        let n = params.items.len();
        if n == 0 && f.statements().is_some_and(<[_]>::is_empty) {
            return;
        }
        if uses_arguments(ctx, f) {
            return;
        }
        let expected = if n == 0 {
            "no arguments".to_string()
        } else {
            plural(n, "argument", "arguments")
        };
        let provided = match call.arguments.len() {
            1 => "1 was".to_string(),
            k => format!("{k} were"),
        };
        let mut secondary = Vec::new();
        if n > 0 {
            secondary.push(sec(
                Span::new(params.items[0].span.start, params.items[n - 1].span.end),
                Some("Formal parameters"),
            ));
        } else {
            secondary.push(sec(params.span, Some("Formal parameters")));
        }
        for a in &call.arguments[n..] {
            secondary.push(sec(a.span(), Some("Extra argument")));
        }
        ctx.report_with(
            call.callee.span(),
            format!("This function expects {expected}, but {provided} provided."),
            secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn extra_arguments() {
        check(
            "S930",
            "a.js",
            r#"
function two(a, b) { return a + b; }
two(1, 2, 3); // Noncompliant {{This function expects 2 arguments, but 3 were provided.}}
two(1, 2);
function none() { return 1; }
none(1); // Noncompliant {{This function expects no arguments, but 1 was provided.}}
function empty() {}
empty(1, 2);
function variadic() { return arguments.length; }
variadic(1, 2);
function rest(a, ...r) { return r; }
rest(1, 2, 3);
const one = (a) => a;
one(1, 2); // Noncompliant {{This function expects 1 argument, but 2 were provided.}}
(function (a) { return a; })(1, 2); // Noncompliant
let reassigned = (a) => a;
reassigned = (a, b) => a + b;
reassigned(1, 2);
unknown(1, 2, 3);
"#,
        );
    }
}
