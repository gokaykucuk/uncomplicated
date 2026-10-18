//! React "render" functions should return a value
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Your render method should have a return statement";

/// True if the render function value has a `return` (or is an expression arrow).
fn render_returns(value: &Expression<'_>) -> bool {
    match strip_parens(value) {
        Expression::FunctionExpression(f) => f
            .body
            .as_ref()
            .is_none_or(|b| any_return(&b.statements, &mut |_| true)),
        Expression::ArrowFunctionExpression(a) => match a.body.as_function_body() {
            Some(b) => any_return(&b.statements, &mut |_| true),
            None => true,
        },
        // Not a function (e.g. `render = someFn`): not checked.
        _ => true,
    }
}

fn check_class(ctx: &Ctx<'_, '_>, class: &Class<'_>) {
    for el in &class.body.body {
        if class_member_name(el) != Some("render") {
            continue;
        }
        match el {
            ClassElement::MethodDefinition(m) if m.kind == MethodDefinitionKind::Method => {
                if let Some(b) = &m.value.body
                    && !any_return(&b.statements, &mut |_| true)
                {
                    ctx.report(m.span, MSG);
                }
            }
            ClassElement::PropertyDefinition(p) => {
                if let Some(v) = &p.value
                    && !render_returns(v)
                {
                    ctx.report(p.span, MSG);
                }
            }
            _ => {}
        }
        return;
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class, AstType::ObjectExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Class(c) if is_es6_component(c) => check_class(ctx, c),
            AstKind::ObjectExpression(o) if is_es5_component(ctx, node.id()) => {
                for p in &o.properties {
                    let ObjectPropertyKind::ObjectProperty(p) = p else {
                        continue;
                    };
                    if property_key_name(&p.key) != Some("render") || p.computed {
                        continue;
                    }
                    if !render_returns(&p.value) {
                        ctx.report(p.span, MSG);
                    }
                    return;
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
    fn render_return() {
        check(
            "S6435",
            "a.js",
            r#"
const React = require('react');
class App extends React.Component {
  render() { // Noncompliant {{Your render method should have a return statement}}
    <div>Contents</div>;
  }
}
class B extends Component {
  render() {
    if (a) { return null; }
    const f = () => { return 1; };
  }
}
class C extends React.PureComponent {
  render() { // Noncompliant
    const f = () => { return <div />; };
    f();
  }
}
class D extends React.Component {
  render = () => <div />;
}
class E extends React.Component {
  render = () => { // Noncompliant
    foo();
  };
}
class NotComponent {
  render() {}
}
const F = createReactClass({
  render: function() { // Noncompliant
    foo();
  },
});
const G = createReactClass({
  render() { return <div />; },
});
"#,
        );
    }
}
