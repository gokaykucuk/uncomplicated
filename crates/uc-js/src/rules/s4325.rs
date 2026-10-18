//! Redundant casts and non-null assertions should be avoided
use crate::helpers::batch_ts::strip_type_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const UNNECESSARY: &str =
    "This assertion is unnecessary since it does not change the type of the expression.";

/// Annotation and "definitely assigned" flag of a local variable or parameter.
fn declared_annotation<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<&'a TSType<'a>> {
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v)
            if matches!(v.id, BindingPattern::BindingIdentifier(_)) && v.init.is_some() =>
        {
            v.type_annotation.as_ref().map(|a| &a.type_annotation)
        }
        AstKind::FormalParameter(p)
            if matches!(p.pattern, BindingPattern::BindingIdentifier(_)) && !p.optional =>
        {
            p.type_annotation.as_ref().map(|a| &a.type_annotation)
        }
        _ => None,
    }
}

/// True if values of this annotated type can never be null or undefined.
fn non_nullable_type(ctx: &Ctx<'_, '_>, t: &TSType<'_>) -> bool {
    match strip_type_parens(t) {
        TSType::TSStringKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSObjectKeyword(_)
        | TSType::TSArrayType(_)
        | TSType::TSTupleType(_)
        | TSType::TSFunctionType(_)
        | TSType::TSTypeLiteral(_)
        | TSType::TSLiteralType(_) => true,
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return false;
            };
            match ctx.scoping().get_reference(id.reference_id()).symbol_id() {
                Some(s) => matches!(
                    ctx.symbol_decl(s).kind(),
                    AstKind::TSInterfaceDeclaration(_) | AstKind::Class(_)
                ),
                None => matches!(
                    id.name.as_str(),
                    "Array"
                        | "ReadonlyArray"
                        | "Map"
                        | "Set"
                        | "WeakMap"
                        | "WeakSet"
                        | "Date"
                        | "RegExp"
                        | "Promise"
                ),
            }
        }
        _ => false,
    }
}

fn non_nullable_expression(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::ArrayExpression(_)
        | Expression::ObjectExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::NewExpression(_)
        | Expression::ThisExpression(_) => true,
        Expression::Identifier(id) => ctx
            .symbol_of(id)
            .and_then(|s| declared_annotation(ctx, s))
            .is_some_and(|t| non_nullable_type(ctx, t)),
        _ => false,
    }
}

/// True if no reference to `sym` appears in a position that could narrow its type.
fn never_narrowed(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    ctx.scoping().get_resolved_references(sym).all(|r| {
        let mut id = r.node_id();
        loop {
            let parent = ctx.parent(id);
            match parent.kind() {
                AstKind::ParenthesizedExpression(_) => id = parent.id(),
                AstKind::BinaryExpression(_)
                | AstKind::LogicalExpression(_)
                | AstKind::UnaryExpression(_)
                | AstKind::IfStatement(_)
                | AstKind::ConditionalExpression(_)
                | AstKind::SwitchStatement(_)
                | AstKind::WhileStatement(_)
                | AstKind::DoWhileStatement(_)
                | AstKind::ForStatement(_) => return false,
                _ => return true,
            }
        }
    })
}

fn primitive_keyword(t: &TSType<'_>) -> Option<&'static str> {
    Some(match strip_type_parens(t) {
        TSType::TSStringKeyword(_) => "string",
        TSType::TSNumberKeyword(_) => "number",
        TSType::TSBooleanKeyword(_) => "boolean",
        TSType::TSBigIntKeyword(_) => "bigint",
        TSType::TSSymbolKeyword(_) => "symbol",
        _ => return None,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::TSNonNullExpression,
            AstType::TSAsExpression,
            AstType::TSTypeAssertion,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::TSNonNullExpression(e) => {
                if let AstKind::AssignmentExpression(_) = ctx.parent_skip_parens(node.id()).kind() {
                    return;
                }
                if non_nullable_expression(ctx, &e.expression) {
                    ctx.report(e.span, UNNECESSARY);
                }
            }
            AstKind::TSAsExpression(_) | AstKind::TSTypeAssertion(_) => {
                let (expr, ty, span) = match node.kind() {
                    AstKind::TSAsExpression(e) => (&e.expression, &e.type_annotation, e.span),
                    AstKind::TSTypeAssertion(e) => (&e.expression, &e.type_annotation, e.span),
                    _ => return,
                };
                let Some(cast) = primitive_keyword(ty) else {
                    return;
                };
                let Expression::Identifier(id) = strip_parens(expr) else {
                    return;
                };
                let Some(sym) = ctx.symbol_of(id) else { return };
                if ctx.scoping().symbol_is_mutated(sym) {
                    return;
                }
                let Some(declared) = declared_annotation(ctx, sym).and_then(primitive_keyword)
                else {
                    return;
                };
                if declared == cast && never_narrowed(ctx, sym) {
                    ctx.report(span, UNNECESSARY);
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
    fn unnecessary() {
        check(
            "S4325",
            "a.ts",
            r#"
function f(s: string, n: number | null, o?: string, b: boolean) {
  const a = s!; // Noncompliant {{This assertion is unnecessary since it does not change the type of the expression.}}
  const c = n!;
  const d = o!;
  const e = s as string; // Noncompliant
  const g = s as any;
  const h = 'x'!; // Noncompliant
  if (b) {
    const k = b as boolean;
  }
}
function g(x: unknown, y: Foo, z: string) {
  const a = x!;
  const b = y!;
  if (z === 'a') {}
  const c = z as string;
}
"#,
        );
    }
}
