//! Methods should not contain selector parameters
//!
//! A boolean parameter is a selector when a function's single statement is an `if/else` (or a
//! returned / evaluated ternary with side effects) testing it. The parameter must be known to be
//! a `boolean` (annotation or boolean default value).
use oxc_ast_visit::Visit;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Function expressions passed as callbacks, JSX attribute values or `onXxx` handlers are skipped.
fn is_callback(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let span = node.kind().span();
    match ctx.parent_kind(node.id()) {
        AstKind::CallExpression(c) => c.arguments.iter().any(|a| a.span() == span),
        AstKind::JSXExpressionContainer(_) => true,
        AstKind::ObjectProperty(p) if !p.computed => {
            let key = match &p.key {
                PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
                PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
                _ => None,
            };
            key.is_some_and(|k| {
                let b = k.as_bytes();
                b.len() > 2 && b[0] == b'o' && b[1] == b'n' && b[2].is_ascii_uppercase()
            })
        }
        _ => false,
    }
}

fn has_side_effects(e: &Expression<'_>) -> bool {
    match e {
        Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::AssignmentExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::YieldExpression(_)
        | Expression::AwaitExpression(_) => true,
        Expression::ParenthesizedExpression(p) => has_side_effects(&p.expression),
        Expression::SequenceExpression(s) => s.expressions.iter().any(has_side_effects),
        Expression::ConditionalExpression(c) => {
            has_side_effects(&c.consequent) || has_side_effects(&c.alternate)
        }
        Expression::LogicalExpression(l) => has_side_effects(&l.left) || has_side_effects(&l.right),
        Expression::BinaryExpression(b) => has_side_effects(&b.left) || has_side_effects(&b.right),
        Expression::UnaryExpression(u) => {
            u.operator == UnaryOperator::Delete || has_side_effects(&u.argument)
        }
        Expression::StaticMemberExpression(m) => has_side_effects(&m.object),
        Expression::ComputedMemberExpression(m) => has_side_effects(&m.object),
        Expression::ChainExpression(_) => true,
        Expression::ArrayExpression(a) => a
            .elements
            .iter()
            .any(|el| el.as_expression().is_some_and(has_side_effects)),
        Expression::ObjectExpression(o) => o.properties.iter().any(|p| match p {
            ObjectPropertyKind::ObjectProperty(p) => has_side_effects(&p.value),
            ObjectPropertyKind::SpreadProperty(_) => false,
        }),
        _ => false,
    }
}

/// Resolved identifier references inside an expression.
struct Idents<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    found: Vec<(Span, SymbolId)>,
}

impl<'a> Visit<'a> for Idents<'_, 'a, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(sym) = self.ctx.symbol_of(it) {
            self.found.push((it.span, sym));
        }
    }
}

/// A boolean parameter of the function: `flag: boolean` or `flag = true`.
fn is_boolean_param(ctx: &Ctx<'_, '_>, sym: SymbolId) -> Option<Span> {
    let AstKind::FormalParameter(p) = ctx.symbol_decl(sym).kind() else {
        return None;
    };
    let BindingPattern::BindingIdentifier(binding) = &p.pattern else {
        return None;
    };
    if p.optional {
        return None;
    }
    let is_bool = match (&p.type_annotation, &p.initializer) {
        (Some(ann), _) => matches!(ann.type_annotation, TSType::TSBooleanKeyword(_)),
        (None, Some(init)) => matches!(strip_parens(init), Expression::BooleanLiteral(_)),
        (None, None) => false,
    };
    is_bool.then_some(binding.span)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let body = match node.kind() {
            AstKind::Function(f) => {
                if !f.is_declaration() && is_callback(ctx, node) {
                    return;
                }
                let Some(body) = &f.body else { return };
                &**body
            }
            AstKind::ArrowFunctionExpression(a) => {
                if is_callback(ctx, node) {
                    return;
                }
                let ArrowFunctionBody::FunctionBody(body) = &a.body else {
                    return;
                };
                &**body
            }
            _ => return,
        };
        let [stmt] = body.statements.as_slice() else {
            return;
        };
        let test = match stmt {
            Statement::IfStatement(i) if i.alternate.is_some() => &i.test,
            Statement::ReturnStatement(r) => match r.argument.as_ref() {
                Some(Expression::ConditionalExpression(c))
                    if has_side_effects(&c.consequent) || has_side_effects(&c.alternate) =>
                {
                    &c.test
                }
                _ => return,
            },
            Statement::ExpressionStatement(e) => match &e.expression {
                Expression::ConditionalExpression(c)
                    if has_side_effects(&c.consequent) || has_side_effects(&c.alternate) =>
                {
                    &c.test
                }
                _ => return,
            },
            _ => return,
        };
        let mut idents = Idents {
            ctx,
            found: Vec::new(),
        };
        idents.visit_expression(test);
        for (span, sym) in idents.found {
            if let Some(decl) = is_boolean_param(ctx, sym) {
                let name = ctx.scoping().symbol_name(sym);
                ctx.report_with(
                    span,
                    format!("Provide multiple methods instead of using \"{name}\" to determine which action to take."),
                    [(decl, Some(format!("Parameter \"{name}\" was declared here")))],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn selector_parameter() {
        check(
            "S2301",
            "a.ts",
            r#"
function tempt(name: string, ofAge: boolean) {
  if (ofAge) { // Noncompliant {{Provide multiple methods instead of using "ofAge" to determine which action to take.}}
    offerLiquor(name);
  } else {
    offerCandy(name);
  }
}
function tempt2(name: string, ofAge = false) {
  return ofAge ? offerLiquor(name) : offerCandy(name); // Noncompliant
}
const tempt3 = (flag: boolean) => {
  flag ? a() : b(); // Noncompliant
};
function label(flag: boolean) {
  return flag ? "Yes" : "No";
}
function noElse(flag: boolean) {
  if (flag) { a(); }
}
function twoStatements(flag: boolean) {
  log();
  if (flag) { a(); } else { b(); }
}
function notBoolean(flag) {
  if (flag) { a(); } else { b(); }
}
items.forEach(function (flag: boolean) { if (flag) { a(); } else { b(); } });
const handlers = { onChange: (v: boolean) => { if (v) { a(); } else { b(); } } };
function destructured({ flag }: { flag: boolean }) {
  if (flag) { a(); } else { b(); }
}
"#,
        );
    }
}
