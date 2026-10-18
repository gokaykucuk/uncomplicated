//! React Context Provider values should have stable identities
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Construct<'a> {
    ty: &'static str,
    span: Span,
    usage: Option<&'a str>,
}

fn construction<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    scope: ScopeId,
    depth: u8,
) -> Option<Construct<'a>> {
    let c = |ty, span| {
        Some(Construct {
            ty,
            span,
            usage: None,
        })
    };
    match strip_parens(e) {
        Expression::RegExpLiteral(r) => c("regular expression", r.span),
        Expression::Identifier(id) => {
            if depth > 4 {
                return None;
            }
            let sym = ctx.scoping().get_binding(scope, id.name)?;
            let decl = ctx.symbol_decl(sym);
            match decl.kind() {
                AstKind::Function(f) if f.is_declaration() => Some(Construct {
                    ty: "function declaration",
                    span: f.span,
                    usage: Some(id.name.as_str()),
                }),
                AstKind::VariableDeclarator(v) => {
                    let inner = construction(ctx, v.init.as_ref()?, scope, depth + 1)?;
                    Some(Construct {
                        usage: Some(id.name.as_str()),
                        ..inner
                    })
                }
                _ => None,
            }
        }
        Expression::ObjectExpression(o) => c("object", o.span),
        Expression::ArrayExpression(a) => c("array", a.span),
        Expression::ArrowFunctionExpression(f) => c("function expression", f.span),
        Expression::FunctionExpression(f) => c("function expression", f.span),
        Expression::ClassExpression(k) => c("class expression", k.span),
        Expression::NewExpression(n) => c("new expression", n.span),
        Expression::ConditionalExpression(x) => construction(ctx, &x.consequent, scope, depth)
            .or_else(|| construction(ctx, &x.alternate, scope, depth)),
        Expression::LogicalExpression(x) => construction(ctx, &x.left, scope, depth)
            .or_else(|| construction(ctx, &x.right, scope, depth)),
        Expression::StaticMemberExpression(m) => {
            let inner = construction(ctx, &m.object, scope, depth)?;
            let usage = match &m.object {
                Expression::Identifier(id) => Some(id.name.as_str()),
                _ => Some("undefined"),
            };
            Some(Construct { usage, ..inner })
        }
        Expression::ComputedMemberExpression(m) => {
            let inner = construction(ctx, &m.object, scope, depth)?;
            let usage = match &m.object {
                Expression::Identifier(id) => Some(id.name.as_str()),
                _ => Some("undefined"),
            };
            Some(Construct { usage, ..inner })
        }
        Expression::JSXFragment(f) => c("JSX fragment", f.span),
        Expression::JSXElement(f) => c("JSX element", f.span),
        Expression::AssignmentExpression(a) => {
            let inner = construction(ctx, &a.right, scope, depth)?;
            Some(Construct {
                ty: "assignment expression",
                span: inner.span,
                usage: Some("undefined"),
            })
        }
        Expression::TSAsExpression(x) => construction(ctx, &x.expression, scope, depth),
        _ => None,
    }
}

fn is_react_context(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
        return false;
    };
    let Some(Expression::CallExpression(c)) = v.init.as_ref() else {
        return false;
    };
    match &c.callee {
        Expression::Identifier(i) => i.name == "createContext",
        Expression::StaticMemberExpression(m) => {
            m.property.name == "createContext"
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let is_ctx = match &el.name {
            JSXElementName::MemberExpression(m) => m.property.name == "Provider",
            JSXElementName::IdentifierReference(id) => is_react_context(ctx, id),
            _ => false,
        };
        if !is_ctx {
            return;
        }
        let Some(value) = find_attr(el, "value").and_then(attr_expr) else {
            return;
        };
        let Some(info) = construction(ctx, value, node.scope_id(), 0) else {
            return;
        };
        if parent_component(ctx, node.id()).is_none() {
            return;
        }
        let hook = if matches!(info.ty, "function expression" | "function declaration") {
            "useCallback"
        } else {
            "useMemo"
        };
        let msg = match info.usage {
            Some(var) => format!(
                "The '{var}' {} passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a {hook} hook.",
                info.ty
            ),
            None => format!(
                "The {} passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a {hook} hook.",
                info.ty
            ),
        };
        ctx.report(info.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn context_values() {
        check(
            "S6481",
            "a.jsx",
            r#"
import React from 'react';
const SomeContext = React.createContext();
const Other = createContext();

function Component() {
  const obj = { a: 1 };
  const fn = () => {}; // Noncompliant
  const memo = useMemo(() => ({}), []);
  return (
    <>
      <SomeContext.Provider value={{foo: 'bar'}}> {/* Noncompliant {{The object passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a useMemo hook.}} */}
        <SomeComponent />
      </SomeContext.Provider>
      <SomeContext.Provider value={memo} />
      <SomeContext.Provider value={fn} />
      <Other value={[1]} /> {/* Noncompliant {{The array passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a useMemo hook.}} */}
      <NotContext value={[1]} />
    </>
  );
}

function Component2() {
  const obj = { a: 1 }; // Noncompliant {{The 'obj' object passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a useMemo hook.}}
  const fn = () => {}; // Noncompliant {{The 'fn' function expression passed as the value prop to the Context provider changes every render. To fix this consider wrapping it in a useCallback hook.}}
  return <div><SomeContext.Provider value={obj} /><Other.Provider value={fn} /></div>;
}

const outside = <SomeContext.Provider value={{ a: 1 }} />;
"#,
        );
    }
}
