//! Default exports should be named
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn function_head(f: &Function<'_>) -> (Span, String) {
    let mut kind = String::new();
    if f.r#async {
        kind.push_str("async ");
    }
    if f.generator {
        kind.push_str("generator ");
    }
    kind.push_str("function");
    (Span::new(f.span.start, f.params.span.start), kind)
}

/// Location and description of an anonymous class or function expression/declaration.
fn anonymous(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<(Span, String)> {
    match strip_parens(e) {
        Expression::FunctionExpression(f) if f.id.is_none() => Some(function_head(f)),
        Expression::ClassExpression(c) if c.id.is_none() => Some(class_head(ctx, c)),
        Expression::ArrowFunctionExpression(a) => {
            let arrow = find_token(
                ctx,
                Span::new(a.params.span.end, a.body.span().start),
                Kind::Arrow,
            )
            .unwrap_or(a.span);
            Some((
                arrow,
                if a.r#async {
                    "async arrow function".into()
                } else {
                    "arrow function".into()
                },
            ))
        }
        _ => None,
    }
}

fn class_head(ctx: &Ctx<'_, '_>, c: &Class<'_>) -> (Span, String) {
    let end = token_before(ctx, c.body.span.start).map_or(c.span.start, |t| t.end);
    (Span::new(c.span.start, end), "class".into())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ExportDefaultDeclaration,
            AstType::AssignmentExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let found = match node.kind() {
            AstKind::ExportDefaultDeclaration(d) => match &d.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) if f.id.is_none() => {
                    Some(function_head(f))
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) if c.id.is_none() => {
                    Some(class_head(ctx, c))
                }
                k => k.as_expression().and_then(|e| anonymous(ctx, e)),
            },
            AstKind::AssignmentExpression(a) => {
                if !matches!(ctx.parent_kind(node.id()), AstKind::ExpressionStatement(_)) {
                    return;
                }
                let target_ok = match &a.left {
                    AssignmentTarget::AssignmentTargetIdentifier(id) => id.name == "exports",
                    AssignmentTarget::StaticMemberExpression(m) => {
                        !m.optional
                            && m.property.name == "exports"
                            && matches!(&m.object, Expression::Identifier(o) if o.name == "module")
                    }
                    _ => false,
                };
                if !target_ok {
                    return;
                }
                anonymous(ctx, &a.right)
            }
            _ => return,
        };
        if let Some((span, desc)) = found {
            ctx.report(span, format!("The {desc} should be named."));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn anonymous_default_export() {
        check(
            "S7726",
            "a.js",
            r#"
export default function () {} // Noncompliant {{The function should be named.}}
"#,
        );
        check(
            "S7726",
            "b.js",
            r#"
export default class {} // Noncompliant {{The class should be named.}}
"#,
        );
        check(
            "S7726",
            "c.js",
            r#"
export default async () => {}; // Noncompliant {{The async arrow function should be named.}}
"#,
        );
        check(
            "S7726",
            "d.js",
            r#"
module.exports = function* () {}; // Noncompliant {{The generator function should be named.}}
exports = class {}; // Noncompliant
module.exports = function named() {};
module.exports = { a: () => {} };
foo = function () {};
const x = (module.exports = () => {});
"#,
        );
        check(
            "S7726",
            "e.js",
            r#"
export default function named() {}
"#,
        );
        check(
            "S7726",
            "f.js",
            r#"
const foo = () => {};
export default foo;
"#,
        );
    }
}
