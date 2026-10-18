//! Wrapper functions around built-in type conversion functions should be avoided
use crate::helpers::batch_smell4::function_name_with_kind;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const COERCIONS: &[&str] = &["String", "Number", "BigInt", "Boolean", "Symbol"];
const BOOLEAN_CALLBACK_METHODS: &[&str] = &[
    "every",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "some",
];

/// The single returned expression of a function body.
fn returned<'b, 'a>(body: &'b FunctionBody<'a>) -> Option<&'b Expression<'a>> {
    match body.statements.as_slice() {
        [Statement::ReturnStatement(r)] => r.argument.as_ref(),
        _ => None,
    }
}

fn is_param_ref(e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == name)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (params, ret, head) = match node.kind() {
            AstKind::Function(f) => {
                if f.r#async || f.generator || f.this_param.is_some() {
                    return;
                }
                if f.return_type
                    .as_ref()
                    .is_some_and(|r| matches!(r.type_annotation, TSType::TSTypePredicate(_)))
                {
                    return;
                }
                let Some(body) = &f.body else { return };
                let head_start = match ctx.parent_kind(node.id()) {
                    AstKind::MethodDefinition(m) => {
                        if matches!(
                            m.kind,
                            MethodDefinitionKind::Constructor | MethodDefinitionKind::Set
                        ) {
                            return;
                        }
                        m.span.start
                    }
                    AstKind::ObjectProperty(p) if p.value.span() == f.span => {
                        if p.kind == PropertyKind::Set {
                            return;
                        }
                        p.span.start
                    }
                    _ => f.span.start,
                };
                (
                    &f.params,
                    returned(body),
                    Span::new(head_start, f.params.span.start.max(head_start)),
                )
            }
            AstKind::ArrowFunctionExpression(a) => {
                if a.r#async {
                    return;
                }
                if a.return_type
                    .as_ref()
                    .is_some_and(|r| matches!(r.type_annotation, TSType::TSTypePredicate(_)))
                {
                    return;
                }
                let ret = match a.get_expression() {
                    Some(e) => Some(e),
                    None => a.get_function_body().and_then(returned),
                };
                (&a.params, ret, function_report_span(ctx, node))
            }
            _ => return,
        };
        let Some(first) = params.items.first() else {
            return;
        };
        if first.initializer.is_some() {
            return;
        }
        let BindingPattern::BindingIdentifier(param) = &first.pattern else {
            return;
        };
        let Some(ret) = ret else { return };
        let name = param.name.as_str();
        let replacement = match strip_parens(ret) {
            Expression::CallExpression(c)
                if !c.optional
                    && c.arguments
                        .first()
                        .and_then(|a| a.as_expression())
                        .is_some_and(|a| is_param_ref(a, name)) =>
            {
                match &c.callee {
                    Expression::Identifier(id)
                        if COERCIONS.contains(&id.name.as_str()) && ctx.symbol_of(id).is_none() =>
                    {
                        id.name.as_str()
                    }
                    _ => return,
                }
            }
            e if is_param_ref(e, name) => {
                // Identity callback of an array predicate method: `array.filter(x => x)`.
                let AstKind::CallExpression(call) = ctx.parent_kind(node.id()) else {
                    return;
                };
                let Expression::StaticMemberExpression(m) = &call.callee else {
                    return;
                };
                let is_first_arg =
                    call.arguments.len() == 1 && call.arguments[0].span() == node.kind().span();
                if !is_first_arg
                    || m.optional
                    || call.optional
                    || !BOOLEAN_CALLBACK_METHODS.contains(&m.property.name.as_str())
                {
                    return;
                }
                "Boolean"
            }
            _ => return,
        };
        let kind = function_name_with_kind(ctx, node);
        ctx.report(
            head,
            format!("{kind} is equivalent to `{replacement}`. Use `{replacement}` directly."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7770",
            "a.ts",
            r#"
const toBoolean = (x: unknown) => Boolean(x); // Noncompliant {{arrow function 'toBoolean' is equivalent to `Boolean`. Use `Boolean` directly.}}
function toNumber(v) { return Number(v); } // Noncompliant {{function 'toNumber' is equivalent to `Number`. Use `Number` directly.}}
const o = { toStr(v) { return String(v); } }; // Noncompliant {{method 'toStr' is equivalent to `String`. Use `String` directly.}}
values.map(v => String(v)); // Noncompliant {{arrow function is equivalent to `String`. Use `String` directly.}}
values.filter(v => v); // Noncompliant {{arrow function is equivalent to `Boolean`. Use `Boolean` directly.}}
const isString = (x: unknown): x is string => Boolean(x);
const a = async (x) => Number(x);
const b = (x) => Number(x, 10).toFixed();
const c = (x, y) => Number(y);
const d = (x = 1) => Number(x);
values.map(v => v);
function e(Number) { return (v) => Number(v); }
"#,
        );
    }
}
