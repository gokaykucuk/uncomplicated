//! Callbacks of array methods should have return statements
use crate::helpers::batch_bug2::{body_facts, is_typed_array_name};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Add a \"return\" statement to this callback.";

const METHODS: [&str; 13] = [
    "every",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "map",
    "flatMap",
    "reduce",
    "reduceRight",
    "some",
    "sort",
    "toSorted",
];

/// True when the function certainly returns `void`.
fn is_void_function(
    is_async: bool,
    generator: bool,
    ret: Option<&TSTypeAnnotation<'_>>,
    body: Option<&FunctionBody<'_>>,
) -> bool {
    if is_async || generator {
        return false;
    }
    if let Some(r) = ret {
        return matches!(r.type_annotation, TSType::TSVoidKeyword(_));
    }
    let Some(body) = body else { return false };
    let facts = body_facts(body);
    // A function that may only throw has return type `never`.
    !facts.returns_value && !facts.throws
}

fn arrow_is_void(a: &ArrowFunctionExpression<'_>) -> bool {
    a.body
        .as_function_body()
        .is_some_and(|b| is_void_function(a.r#async, false, a.return_type.as_deref(), Some(b)))
}

fn func_is_void(f: &Function<'_>) -> bool {
    is_void_function(
        f.r#async,
        f.generator,
        f.return_type.as_deref(),
        f.body.as_deref(),
    )
}

/// Location to report for a void callback, if `arg` is one.
fn void_callback_span(ctx: &Ctx<'_, '_>, arg: &Argument<'_>) -> Option<Span> {
    let e = arg.as_expression()?;
    match strip_parens(e) {
        Expression::ArrowFunctionExpression(a) if arrow_is_void(a) => {
            let end = a.body.span().start.max(a.params.span.end);
            Some(find_token(ctx, Span::new(a.params.span.end, end), Kind::Arrow).unwrap_or(a.span))
        }
        Expression::FunctionExpression(f) if func_is_void(f) => Some(match &f.id {
            Some(id) => id.span,
            None => find_token(ctx, f.span, Kind::Function).unwrap_or(f.span),
        }),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            let void = match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) => func_is_void(f),
                AstKind::VariableDeclarator(v) if !ctx.scoping().symbol_is_mutated(sym) => {
                    match v.init.as_ref().map(strip_parens) {
                        Some(Expression::ArrowFunctionExpression(a)) => arrow_is_void(a),
                        Some(Expression::FunctionExpression(f)) => func_is_void(f),
                        _ => false,
                    }
                }
                _ => false,
            };
            void.then_some(id.span)
        }
        _ => None,
    }
}

fn is_array_like(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let t = type_of(ctx, e);
    t.is_array() || matches!(&t, Ty::Named(n) if is_typed_array_name(n))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !crate::helpers::batch_bug2::has_type_info(ctx) {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(member) = call.callee.as_member_expression() else {
            return;
        };
        let Some(name) = member.static_property_name() else {
            return;
        };
        if call.arguments.is_empty() {
            return;
        }
        if METHODS.contains(&name) && is_array_like(ctx, member.object()) {
            if let Some(span) = void_callback_span(ctx, &call.arguments[0]) {
                ctx.report(span, MSG);
            }
        } else if name == "from"
            && call.arguments.len() > 1
            && matches!(member.object(), Expression::Identifier(id) if ctx.is_global(id, "Array"))
            && let Some(span) = void_callback_span(ctx, &call.arguments[1])
        {
            ctx.report(span, MSG);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn callbacks() {
        check(
            "S3796",
            "a.js",
            r#"
const arr = [1, 2, 3];
arr.map(x => { x * 2; }); // Noncompliant {{Add a "return" statement to this callback.}}
arr.filter(function (x) { console.log(x); }); // Noncompliant
arr.every(function named(x) { foo(x); }); // Noncompliant
arr["some"]((x) => { if (x) { return; } }); // Noncompliant
Array.from(arr, x => { x; }); // Noncompliant
function cb(x) { bar(x); }
arr.sort(cb); // Noncompliant
arr.map(x => x * 2);
arr.map(x => { return x * 2; });
arr.map(async x => { await x; });
arr.map(x => { throw new Error(); });
arr.forEach(x => { x * 2; });
unknown.map(x => { x * 2; });
arr.map(function* (x) { yield x; });
arr.map(x => { function inner() { return 1; } inner(); }); // Noncompliant
Array.from(arr);
"#,
        );
    }
}
