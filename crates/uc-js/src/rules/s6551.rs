//! Objects and classes converted or coerced to strings should define a "toString()" method
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn object_literal_uses_base_to_string(o: &ObjectExpression<'_>) -> bool {
    o.properties.iter().all(|p| match p {
        ObjectPropertyKind::ObjectProperty(p) => {
            !p.computed
                && !matches!(
                    property_key_name(&p.key),
                    Some("toString" | "__proto__" | "toPrimitive")
                )
        }
        ObjectPropertyKind::SpreadProperty(_) => false,
    })
}

fn class_uses_base_to_string(c: &Class<'_>) -> bool {
    c.heritage.is_none()
        && c.decorators.is_empty()
        && c.body.body.iter().all(|e| match e {
            ClassElement::MethodDefinition(m) => {
                m.computed || property_key_name(&m.key) != Some("toString")
            }
            ClassElement::PropertyDefinition(p) => {
                p.computed || property_key_name(&p.key) != Some("toString")
            }
            ClassElement::AccessorProperty(p) => {
                p.computed || property_key_name(&p.key) != Some("toString")
            }
            ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => true,
        })
}

/// The expression certainly evaluates to an object whose `toString` is `Object.prototype`'s.
fn uses_base_to_string(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> bool {
    if depth > 3 {
        return false;
    }
    match strip_parens(e) {
        Expression::ObjectExpression(o) => object_literal_uses_base_to_string(o),
        Expression::NewExpression(n) => {
            let Expression::Identifier(id) = &n.callee else {
                return false;
            };
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            if ctx.scoping().symbol_is_mutated(sym) {
                return false;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::Class(c) => class_uses_base_to_string(c),
                _ => false,
            }
        }
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            if ctx.scoping().symbol_is_mutated(sym) {
                return false;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) if v.type_annotation.is_none() => {
                    matches!(v.id, BindingPattern::BindingIdentifier(_))
                        && v.init
                            .as_ref()
                            .is_some_and(|i| uses_base_to_string(ctx, i, depth + 1))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn check(ctx: &Ctx<'_, '_>, e: &Expression<'_>) {
    if uses_base_to_string(ctx, e, 0) {
        let e = strip_parens(e);
        ctx.report(
            e.span(),
            format!(
                "'{}' will use Object's default stringification format ('[object Object]') when stringified.",
                ctx.text(e.span())
            ),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::TemplateLiteral,
            AstType::BinaryExpression,
            AstType::AssignmentExpression,
            AstType::CallExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::TemplateLiteral(t) => {
                if matches!(
                    ctx.parent_kind(node.id()),
                    AstKind::TaggedTemplateExpression(_)
                ) {
                    return;
                }
                for e in &t.expressions {
                    check(ctx, e);
                }
            }
            AstKind::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
                if type_of(ctx, &b.left) == Ty::String {
                    check(ctx, &b.right);
                } else if type_of(ctx, &b.right) == Ty::String {
                    check(ctx, &b.left);
                }
            }
            AstKind::AssignmentExpression(a) if a.operator == AssignmentOperator::Addition => {
                if helpers::batch_smell3::target_type(ctx, &a.left) == Ty::String {
                    check(ctx, &a.right);
                }
            }
            AstKind::CallExpression(c) => match &c.callee {
                Expression::Identifier(id) if ctx.is_global(id, "String") => {
                    if let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) {
                        check(ctx, arg);
                    }
                }
                Expression::StaticMemberExpression(m)
                    if matches!(m.property.name.as_str(), "toString" | "toLocaleString") =>
                {
                    check(ctx, &m.object);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn base_to_string() {
        check(
            "S6551",
            "a.ts",
            r#"
const obj = { a: 1 };
const s1 = `value: ${obj}`;     // Noncompliant {{'obj' will use Object's default stringification format ('[object Object]') when stringified.}}
const s2 = 'x' + obj;           // Noncompliant
const s3 = String(obj);         // Noncompliant
const s4 = obj.toString();      // Noncompliant
class Foo { a = 1; }
const foo = new Foo();
const s5 = `${foo}`;            // Noncompliant
class Bar { toString() { return 'bar'; } }
const bar = new Bar();
const s6 = `${bar}`;
const custom = { toString() { return 'c'; } };
const s7 = `${custom}`;
const s8 = `${unknown}`;
const n = 1;
const s9 = `${n}`;
const s10 = tag`${obj}`;
"#,
        );
    }
}
