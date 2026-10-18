//! Disallow `.bind()` and arrow functions in JSX props
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    ignore_refs: bool,
    ignore_dom: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            ignore_refs: p.bool("ignoreRefs", true),
            ignore_dom: p.bool("ignoreDOMComponents", true),
        }
    }
}

#[derive(Clone, Copy)]
enum Violation {
    Bind,
    Arrow,
    Func,
}

impl Violation {
    fn msg(self) -> &'static str {
        match self {
            Violation::Bind => "JSX props should not use .bind()",
            Violation::Arrow => "JSX props should not use arrow functions",
            Violation::Func => "JSX props should not use functions",
        }
    }
}

fn violation(e: &Expression<'_>) -> Option<Violation> {
    match strip_parens(e) {
        Expression::CallExpression(c) if matches!(&c.callee, Expression::StaticMemberExpression(m) if m.property.name == "bind") => {
            Some(Violation::Bind)
        }
        Expression::ConditionalExpression(c) => violation(&c.test)
            .or_else(|| violation(&c.consequent))
            .or_else(|| violation(&c.alternate)),
        Expression::ArrowFunctionExpression(_) => Some(Violation::Arrow),
        Expression::FunctionExpression(_) => Some(Violation::Func),
        _ => None,
    }
}

/// ESTree `BlockStatement`s: blocks and function bodies (not expression arrow bodies).
fn is_block(n: &AstNode<'_>) -> bool {
    matches!(
        n.kind(),
        AstKind::BlockStatement(_) | AstKind::FunctionBody(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(attr) = node.kind() else {
            return;
        };
        if self.ignore_refs && attr_name(attr) == Some("ref") {
            return;
        }
        let Some(value) = attr_expr(attr) else { return };
        if self.ignore_dom
            && let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id())
            && is_dom_component(ctx, el)
        {
            return;
        }
        if let Expression::Identifier(id) = value {
            let Some(sym) = ctx.symbol_of(id) else { return };
            let decl = ctx.symbol_decl(sym);
            if decl.kind().span().start > attr.span.start {
                return;
            }
            let v = match decl.kind() {
                AstKind::VariableDeclarator(d) => {
                    let is_const = matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(vd) if vd.kind == VariableDeclarationKind::Const);
                    if !is_const || !matches!(d.id, BindingPattern::BindingIdentifier(_)) {
                        return;
                    }
                    d.init.as_ref().and_then(violation)
                }
                AstKind::Function(f) if f.is_declaration() => Some(Violation::Func),
                _ => None,
            };
            let Some(v) = v else { return };
            // The declaration must sit in a block that encloses the attribute.
            let Some(block) = ancestors(ctx, decl.id()).find(|n| is_block(n)) else {
                return;
            };
            if ancestors(ctx, node.id()).any(|n| n.id() == block.id()) {
                ctx.report(attr.span, v.msg());
            }
        } else if let Some(v) = violation(value) {
            ctx.report(attr.span, v.msg());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn no_bind() {
        check(
            "S6480",
            "a.jsx",
            r#"
function Comp() {
  const handler = () => {};
  const bound = this.foo.bind(this);
  function fn() {}
  let notConst = () => {};
  const ok = useCallback(() => {}, []);
  return (
    <div onClick={() => {}}>
      <Foo onClick={() => {}} /> {/* Noncompliant {{JSX props should not use arrow functions}} */}
      <Foo onClick={this.bar.bind(this)} /> {/* Noncompliant {{JSX props should not use .bind()}} */}
      <Foo onClick={function () {}} /> {/* Noncompliant {{JSX props should not use functions}} */}
      <Foo onClick={handler} /> {/* Noncompliant */}
      <Foo onClick={bound} /> {/* Noncompliant */}
      <Foo onClick={fn} /> {/* Noncompliant */}
      <Foo onClick={notConst} />
      <Foo onClick={ok} />
      <Foo ref={() => {}} />
      <Foo onClick={cond ? () => {} : null} /> {/* Noncompliant */}
    </div>
  );
}
const top = () => {};
const T = () => <Foo onClick={top} />;
"#,
        );
    }
}
