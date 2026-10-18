//! Lodash and Underscore.js debounced or throttled functions should not be recreated on every React render
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn supported_method(fqn: &str) -> Option<&'static str> {
    let normalized = fqn.replace('/', ".");
    let mut parts = normalized.split('.');
    let (Some(module), Some(q), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if !matches!(module, "lodash" | "lodash-es" | "underscore") {
        return None;
    }
    match q {
        "debounce" => Some("debounce"),
        "throttle" => Some("throttle"),
        _ => None,
    }
}

fn is_react_class(c: &Class<'_>) -> bool {
    c.heritage
        .as_ref()
        .is_some_and(|h| match strip_parens(&h.expression) {
            Expression::Identifier(id) => matches!(id.name.as_str(), "Component" | "PureComponent"),
            Expression::StaticMemberExpression(m) => {
                matches!(m.property.name.as_str(), "Component" | "PureComponent")
                    && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
            }
            _ => false,
        })
}

fn is_wrapper_call(c: &CallExpression<'_>) -> bool {
    match &c.callee {
        Expression::Identifier(id) => matches!(id.name.as_str(), "memo" | "forwardRef"),
        Expression::StaticMemberExpression(m) => {
            matches!(m.property.name.as_str(), "memo" | "forwardRef")
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        _ => false,
    }
}

/// Sonar's `getComponentIdentifier` name.
fn component_identifier<'a>(ctx: &Ctx<'a, '_>, func: &AstNode<'a>) -> Option<&'a str> {
    let mut cur = func;
    let mut parent = estree_parent(ctx, cur.id());
    while let AstKind::CallExpression(c) = parent.kind() {
        if !is_wrapper_call(c) {
            break;
        }
        cur = parent;
        parent = estree_parent(ctx, cur.id());
    }
    let declarator = match parent.kind() {
        AstKind::VariableDeclarator(v) => match &v.id {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    };
    let own = match func.kind() {
        AstKind::Function(f) => f.id.as_ref().map(|i| i.name.as_str()),
        _ => None,
    };
    let direct_parent_is_call = matches!(
        estree_parent(ctx, func.id()).kind(),
        AstKind::CallExpression(_)
    );
    if declarator.is_some()
        && (matches!(func.kind(), AstKind::ArrowFunctionExpression(_))
            || own.is_none()
            || direct_parent_is_call)
    {
        return declarator;
    }
    own.or(declarator)
}

#[derive(PartialEq)]
enum Component {
    Function,
    Class,
}

fn component_type<'a>(ctx: &Ctx<'a, '_>, func: &AstNode<'a>) -> Option<Component> {
    if let Some(name) = component_identifier(ctx, func)
        && (name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || (name.len() > 3
                && name.starts_with("use")
                && name.as_bytes()[3].is_ascii_uppercase()))
    {
        return Some(Component::Function);
    }
    let member = ctx.parent(func.id());
    let is_render = match member.kind() {
        AstKind::MethodDefinition(m) => {
            !m.computed
                && !m.r#static
                && matches!(&m.key, PropertyKey::StaticIdentifier(k) if k.name == "render")
        }
        AstKind::PropertyDefinition(p) => {
            !p.computed
                && !p.r#static
                && matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "render")
        }
        _ => false,
    };
    if is_render
        && ancestors(ctx, member.id()).find_map(|a| match a.kind() {
            AstKind::Class(c) => Some(is_react_class(c)),
            _ => None,
        }) == Some(true)
    {
        return Some(Component::Class);
    }
    None
}

fn is_use_ref_call(ctx: &Ctx<'_, '_>, c: &CallExpression<'_>) -> bool {
    fqn(ctx, &c.callee).as_deref() == Some("react.useRef")
}

fn is_ref_current(e: &Expression<'_>, ref_name: Option<&str>) -> bool {
    matches!(strip_parens(e), Expression::StaticMemberExpression(m)
        if m.property.name == "current"
        && matches!(&m.object, Expression::Identifier(o) if ref_name.is_none_or(|r| r == o.name)))
}

fn is_ref_lazy_init<'a>(ctx: &Ctx<'a, '_>, call: &AstNode<'a>) -> bool {
    let assign = ctx.parent(call.id());
    let AstKind::AssignmentExpression(a) = assign.kind() else {
        return false;
    };
    if a.operator != AssignmentOperator::Assign {
        return false;
    }
    let Some(MemberExpression::StaticMemberExpression(m)) = a.left.as_member_expression() else {
        return false;
    };
    let Expression::Identifier(ref_id) = &m.object else {
        return false;
    };
    if m.property.name != "current" {
        return false;
    }
    let is_ref = ctx.symbol_of(ref_id).is_some_and(|s| match ctx.symbol_decl(s).kind() {
        AstKind::VariableDeclarator(v) => {
            matches!(v.init.as_ref(), Some(Expression::CallExpression(c)) if is_use_ref_call(ctx, c))
        }
        _ => false,
    });
    if !is_ref {
        return false;
    }
    let stmt = ctx.parent(assign.id());
    if !matches!(stmt.kind(), AstKind::ExpressionStatement(_)) {
        return false;
    }
    let mut p = ctx.parent(stmt.id());
    if let AstKind::BlockStatement(_) = p.kind() {
        p = ctx.parent(p.id());
    }
    let AstKind::IfStatement(i) = p.kind() else {
        return false;
    };
    if !(i.consequent.span().start <= stmt.kind().span().start
        && stmt.kind().span().end <= i.consequent.span().end)
    {
        return false;
    }
    let name = Some(ref_id.name.as_str());
    match &i.test {
        Expression::UnaryExpression(u) => {
            u.operator == UnaryOperator::LogicalNot && is_ref_current(&u.argument, name)
        }
        Expression::BinaryExpression(b)
            if matches!(
                b.operator,
                BinaryOperator::StrictEquality | BinaryOperator::Equality
            ) =>
        {
            (is_ref_current(&b.left, name) && is_null(&b.right))
                || (is_null(&b.left) && is_ref_current(&b.right, name))
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        // Syntactic pre-check of the callee.
        let report_span = match &call.callee {
            Expression::StaticMemberExpression(m) => {
                if !matches!(m.property.name.as_str(), "debounce" | "throttle") {
                    return;
                }
                if matches!(m.object, Expression::CallExpression(_))
                    && require_module(&m.object).is_none()
                {
                    return;
                }
                m.property.span
            }
            Expression::Identifier(id) => id.span,
            Expression::CallExpression(c) => c.span,
            _ => return,
        };
        let Some(method) = fqn(ctx, &call.callee).as_deref().and_then(supported_method) else {
            return;
        };
        let Some(func) = enclosing_function(ctx, node.id()) else {
            return;
        };
        let Some(kind) = component_type(ctx, func) else {
            return;
        };
        // Wrapped in useMemo / useCallback before reaching the component function.
        let memoized = ancestors(ctx, node.id())
            .take_while(|a| a.id() != func.id())
            .any(|a| match a.kind() {
                AstKind::CallExpression(c) => {
                    let name = match &c.callee {
                        Expression::Identifier(id) => Some(id.name.to_string()),
                        _ => fqn(ctx, &c.callee)
                            .map(|f| f.strip_prefix("react.").map(str::to_string).unwrap_or(f)),
                    };
                    matches!(name.as_deref(), Some("useMemo" | "useCallback"))
                }
                _ => false,
            });
        if memoized || is_ref_lazy_init(ctx, node) {
            return;
        }
        // `useRef(debounce(...))`
        if let Some(outer) = ancestors(ctx, node.id())
            .take_while(|a| a.id() != func.id())
            .find(|a| matches!(a.kind(), AstKind::CallExpression(_)))
            && let AstKind::CallExpression(c) = outer.kind()
            && c.arguments
                .first()
                .and_then(Argument::as_expression)
                .is_some_and(|a| strip_ts(a).span() == call.span)
            && is_use_ref_call(ctx, c)
        {
            return;
        }
        let (k, purpose) = if method == "debounce" {
            ("debounced", "debouncing")
        } else {
            ("throttled", "throttling")
        };
        let msg = if kind == Component::Function {
            format!(
                "This {k} function is recreated on every render, which resets its timer and defeats {purpose}. Move it outside the component or hook, or wrap it in useMemo."
            )
        } else {
            format!(
                "This {k} function is recreated on every render, which resets its timer and defeats {purpose}. Initialize it in the constructor or as an instance property."
            )
        };
        report_react(ctx, report_span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn debounce_in_render() {
        check(
            "S9114",
            "a.jsx",
            r#"
import { debounce, throttle } from 'lodash';
import React, { useMemo, useRef } from 'react';
import _ from 'underscore';

function Search() {
  const onChange = debounce(fetchResults, 300); // Noncompliant {{This debounced function is recreated on every render, which resets its timer and defeats debouncing. Move it outside the component or hook, or wrap it in useMemo.}}
  const onScroll = _.throttle(update, 100); // Noncompliant {{This throttled function is recreated on every render, which resets its timer and defeats throttling. Move it outside the component or hook, or wrap it in useMemo.}}
  return <input onChange={onChange} />;
}
function CompliantSearch() {
  const onChange = useMemo(() => debounce(fetchResults, 300), []);
  const cb = React.useCallback(debounce(fetchResults, 300), []);
  const r = useRef(debounce(fetchResults, 300));
  return <input onChange={onChange} />;
}
function useDebounced(onSearch) {
  const ref = useRef(null);
  if (!ref.current) {
    ref.current = debounce(onSearch, 300);
  }
  return debounce(onSearch, 300); // Noncompliant
}
const Memo = React.memo(function Search() {
  const onChange = debounce(fetchResults, 300); // Noncompliant
  return null;
});
class ClassSearch extends React.Component {
  constructor(props) { super(props); this.onChange = debounce(fetchResults, 300); }
  render() {
    const onChange = debounce(fetchResults, 300); // Noncompliant {{This debounced function is recreated on every render, which resets its timer and defeats debouncing. Initialize it in the constructor or as an instance property.}}
    return <input onChange={onChange} />;
  }
}
function helper() { return debounce(f, 1); }
const top = debounce(f, 1);
function Other() { const x = myLib.debounce(f, 1); return null; }
"#,
        );
    }
}
