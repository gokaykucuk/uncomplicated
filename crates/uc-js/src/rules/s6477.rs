//! JSX list components should have a key property
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ITER: &str = "Missing \"key\" prop for element in iterator";
const ARRAY: &str = "Missing \"key\" prop for element in array";

/// `jsx-ast-utils` `hasProp(attrs, 'key')` (case-insensitive, spreads ignored).
fn has_key(el: &JSXOpeningElement<'_>) -> bool {
    find_attr_ignore_case(el, "key").is_some()
}

fn check_element(ctx: &Ctx<'_, '_>, e: &Expression<'_>, msg: &str) {
    if let Expression::JSXElement(el) = strip_parens(e)
        && !has_key(&el.opening_element)
        // Sonar: elements with a spread attribute may receive the key through it.
        && !has_spread_attr(&el.opening_element)
    {
        ctx.report(el.span, msg);
    }
}

fn collect_returns<'b, 'a>(stmts: &'b [Statement<'a>], out: &mut Vec<&'b Expression<'a>>) {
    for s in stmts {
        match s {
            Statement::IfStatement(i) => if_returns(i, out),
            Statement::ReturnStatement(r) => out.extend(r.argument.as_ref()),
            _ => {}
        }
    }
}

fn if_returns<'b, 'a>(i: &'b IfStatement<'a>, out: &mut Vec<&'b Expression<'a>>) {
    for s in std::iter::once(&i.consequent).chain(i.alternate.as_ref()) {
        match s {
            Statement::IfStatement(i) => if_returns(i, out),
            Statement::ReturnStatement(r) => out.extend(r.argument.as_ref()),
            Statement::BlockStatement(b) => collect_returns(&b.body, out),
            _ => {}
        }
    }
}

fn check_callback(ctx: &Ctx<'_, '_>, f: &Expression<'_>) {
    match strip_parens(f) {
        Expression::ArrowFunctionExpression(a) => match a.get_expression() {
            Some(body) => match strip_parens(body) {
                Expression::JSXElement(_) => check_element(ctx, body, ITER),
                Expression::ConditionalExpression(c) => {
                    check_element(ctx, &c.consequent, ITER);
                    check_element(ctx, &c.alternate, ITER);
                }
                Expression::LogicalExpression(l) => check_element(ctx, &l.right, ITER),
                _ => {}
            },
            None => {
                if let Some(b) = a.get_function_body() {
                    let mut out = Vec::new();
                    collect_returns(&b.statements, &mut out);
                    out.into_iter().for_each(|e| check_element(ctx, e, ITER));
                }
            }
        },
        Expression::FunctionExpression(f) => {
            if let Some(b) = &f.body {
                let mut out = Vec::new();
                collect_returns(&b.statements, &mut out);
                out.into_iter().for_each(|e| check_element(ctx, e, ITER));
            }
        }
        _ => {}
    }
}

fn is_children_to_array(call: &CallExpression<'_>) -> bool {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return false;
    };
    if m.property.name != "toArray" {
        return false;
    }
    match &m.object {
        Expression::Identifier(o) => o.name == "Children",
        Expression::StaticMemberExpression(o) => {
            o.property.name == "Children"
                && matches!(&o.object, Expression::Identifier(r) if r.name == "React")
        }
        _ => false,
    }
}

fn within_children_to_array(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    ancestors(ctx, id)
        .any(|a| matches!(a.kind(), AstKind::CallExpression(c) if is_children_to_array(c)))
}

fn member_property<'a>(callee: &Expression<'a>) -> Option<&'a str> {
    match callee {
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(m) => Some(m.property.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ArrayExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ArrayExpression(arr) => {
                let jsx = || {
                    arr.elements
                        .iter()
                        .filter_map(|e| match e.as_expression().map(strip_parens) {
                            Some(Expression::JSXElement(el)) => Some(el),
                            _ => None,
                        })
                };
                if jsx().next().is_none() || within_children_to_array(ctx, node.id()) {
                    return;
                }
                for el in jsx() {
                    if find_attr(&el.opening_element, "key").is_none()
                        && !has_spread_attr(&el.opening_element)
                    {
                        ctx.report(el.span, ARRAY);
                    }
                }
            }
            AstKind::CallExpression(call) => {
                let cb = match member_property(&call.callee) {
                    Some("map") => call.arguments.first(),
                    Some("from") => call.arguments.get(1),
                    _ => return,
                };
                let Some(cb) = cb.and_then(Argument::as_expression) else {
                    return;
                };
                if !matches!(
                    strip_parens(cb),
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                ) {
                    return;
                }
                if within_children_to_array(ctx, node.id()) {
                    return;
                }
                check_callback(ctx, cb);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn missing_key() {
        check(
            "S6477",
            "a.tsx",
            r#"
const React = require('react');
const a = [
  <App key="unique id" />,
  <App {...props} />,
  <App />, // Noncompliant {{Missing "key" prop for element in array}}
];
function Blog(props) {
  return (
    <ul>
      {props.posts.map((post) =>
        <li> {/* Noncompliant {{Missing "key" prop for element in iterator}} */}
          {post.title}
        </li>
      )}
      {props.posts.map(post => <li key={post.id}>{post.title}</li>)}
      {props.posts?.map(post => cond ? <li /> : <b key="x" />)} {/* Noncompliant */}
      {props.posts.map(function (post) {
        if (post.x) {
          return <i />; // Noncompliant
        }
        return <li key={post.id} />;
      })}
      {Array.from(items, (x) => <li />)} {/* Noncompliant */}
      {React.Children.toArray([<a />, <b />])}
    </ul>
  );
}
"#,
        );
    }
}
