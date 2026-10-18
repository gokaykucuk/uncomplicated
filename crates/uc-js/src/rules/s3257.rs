//! Primitive types should be omitted from initialized or defaulted declarations
use crate::helpers::batch_ts::strip_type_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_call_to(e: &Expression<'_>, name: &str) -> bool {
    let call = match e {
        Expression::CallExpression(c) => c,
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::CallExpression(c) => c,
            _ => return false,
        },
        _ => return false,
    };
    matches!(&call.callee, Expression::Identifier(id) if id.name == name)
}

fn unwrap_unary<'b, 'a>(e: &'b Expression<'a>, ops: &[UnaryOperator]) -> &'b Expression<'a> {
    match e {
        Expression::UnaryExpression(u) if ops.contains(&u.operator) => strip_parens(&u.argument),
        _ => e,
    }
}

fn is_ident(e: &Expression<'_>, names: &[&str]) -> bool {
    matches!(e, Expression::Identifier(id) if names.contains(&id.name.as_str()))
}

/// The inferrable type name, if `init` trivially has the annotated type `ty`.
fn inferrable(ty: &TSType<'_>, init: &Expression<'_>) -> Option<&'static str> {
    let init = strip_parens(init);
    let ok = |b: bool, name: &'static str| b.then_some(name);
    match strip_type_parens(ty) {
        TSType::TSBigIntKeyword(_) => {
            let e = unwrap_unary(init, &[UnaryOperator::UnaryNegation]);
            ok(
                is_call_to(e, "BigInt") || matches!(e, Expression::BigIntLiteral(_)),
                "bigint",
            )
        }
        TSType::TSBooleanKeyword(_) => ok(
            matches!(init, Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot)
                || is_call_to(init, "Boolean")
                || matches!(init, Expression::BooleanLiteral(_)),
            "boolean",
        ),
        TSType::TSNumberKeyword(_) => {
            let e = unwrap_unary(
                init,
                &[UnaryOperator::UnaryNegation, UnaryOperator::UnaryPlus],
            );
            ok(
                is_ident(e, &["Infinity", "NaN"])
                    || is_call_to(e, "Number")
                    || matches!(e, Expression::NumericLiteral(_)),
                "number",
            )
        }
        TSType::TSNullKeyword(_) => ok(matches!(init, Expression::NullLiteral(_)), "null"),
        TSType::TSStringKeyword(_) => ok(
            is_call_to(init, "String")
                || matches!(
                    init,
                    Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
                ),
            "string",
        ),
        TSType::TSSymbolKeyword(_) => ok(is_call_to(init, "Symbol"), "symbol"),
        TSType::TSUndefinedKeyword(_) => ok(
            matches!(init, Expression::UnaryExpression(u) if u.operator == UnaryOperator::Void)
                || is_ident(init, &["undefined"]),
            "undefined",
        ),
        TSType::TSTypeReference(r) if matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == "RegExp") =>
        {
            let is_regexp_ctor = match init {
                Expression::NewExpression(n) => is_ident(&n.callee, &["RegExp"]),
                e => is_call_to(e, "RegExp"),
            };
            ok(
                matches!(init, Expression::RegExpLiteral(_)) || is_regexp_ctor,
                "RegExp",
            )
        }
        _ => None,
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, ty: &TSType<'_>, init: &Expression<'_>) {
    if let Some(name) = inferrable(ty, init) {
        ctx.report(
            span,
            format!(
                "Type {name} trivially inferred from a {name} literal, remove type annotation."
            ),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::VariableDeclarator,
            AstType::PropertyDefinition,
            AstType::AccessorProperty,
            AstType::Function,
            AstType::ArrowFunctionExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::VariableDeclarator(d) => {
                if let (Some(ann), Some(init)) = (&d.type_annotation, &d.init) {
                    report(ctx, d.span, &ann.type_annotation, init);
                }
            }
            AstKind::PropertyDefinition(p) => {
                if p.readonly || p.optional {
                    return;
                }
                if let (Some(ann), Some(init)) = (&p.type_annotation, &p.value) {
                    report(ctx, p.span, &ann.type_annotation, init);
                }
            }
            AstKind::AccessorProperty(p) => {
                if let (Some(ann), Some(init)) = (&p.type_annotation, &p.value) {
                    report(ctx, p.span, &ann.type_annotation, init);
                }
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                let params = match node.kind() {
                    AstKind::Function(f) => &f.params,
                    AstKind::ArrowFunctionExpression(a) => &a.params,
                    _ => return,
                };
                for p in &params.items {
                    if let (Some(ann), Some(init)) = (&p.type_annotation, &p.initializer) {
                        report(
                            ctx,
                            Span::new(p.pattern.span().start, init.span().end),
                            &ann.type_annotation,
                            init,
                        );
                    }
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
    fn inferrable() {
        check(
            "S3257",
            "a.ts",
            r#"
const a: number = 5; // Noncompliant {{Type number trivially inferred from a number literal, remove type annotation.}}
const b: number = -Infinity; // Noncompliant
const c: string = `x`; // Noncompliant {{Type string trivially inferred from a string literal, remove type annotation.}}
const d: boolean = !x; // Noncompliant
const e: RegExp = /a/; // Noncompliant {{Type RegExp trivially inferred from a RegExp literal, remove type annotation.}}
const f: null = null; // Noncompliant
const g: undefined = void 0; // Noncompliant
const h: bigint = 10n; // Noncompliant
const i: symbol = Symbol('x'); // Noncompliant
function fn(p: number = 5, q: string) {} // Noncompliant
class C {
  x: number = 1; // Noncompliant
  readonly y: number = 1;
  z?: number = 1;
  constructor(private w: number = 1) {} // Noncompliant
}
const ok1: number = foo();
const ok2: string = 5 as any;
const ok3 = 5;
let ok4: number;
const ok5: Foo = new Foo();
"#,
        );
    }
}
