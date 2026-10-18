//! Unused methods of React components should be removed
use std::borrow::Cow;

use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const LIFECYCLE: &[&str] = &[
    "constructor",
    "componentDidCatch",
    "componentDidMount",
    "componentDidUpdate",
    "componentWillMount",
    "componentWillReceiveProps",
    "componentWillUnmount",
    "componentWillUpdate",
    "getChildContext",
    "getSnapshotBeforeUpdate",
    "render",
    "shouldComponentUpdate",
    "UNSAFE_componentWillMount",
    "UNSAFE_componentWillReceiveProps",
    "UNSAFE_componentWillUpdate",
];

/// Name of a literal-like key (`foo`, `'foo'`, `1`, `` `foo` ``).
fn key_name<'a>(key: &PropertyKey<'a>, computed: bool) -> Option<Cow<'a, str>> {
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => Some(Cow::Borrowed(id.name.as_str())),
        PropertyKey::StringLiteral(s) => Some(Cow::Borrowed(s.value.as_str())),
        PropertyKey::NumericLiteral(n) => Some(Cow::Owned(n.value.to_string())),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .map(|q| Cow::Borrowed(q.value.raw.as_str())),
        _ => None,
    }
}

/// Name of `this.x` / `this['x']`, if `m` is such a member expression.
fn this_member_name<'a>(kind: &AstKind<'a>) -> Option<Cow<'a, str>> {
    match kind {
        AstKind::StaticMemberExpression(m)
            if matches!(strip_ts(&m.object), Expression::ThisExpression(_)) =>
        {
            Some(Cow::Borrowed(m.property.name.as_str()))
        }
        AstKind::ComputedMemberExpression(m)
            if matches!(strip_ts(&m.object), Expression::ThisExpression(_)) =>
        {
            match &m.expression {
                Expression::StringLiteral(s) => Some(Cow::Borrowed(s.value.as_str())),
                Expression::NumericLiteral(n) => Some(Cow::Owned(n.value.to_string())),
                Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
                    .quasis
                    .first()
                    .map(|q| Cow::Borrowed(q.value.raw.as_str())),
                _ => None,
            }
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class, AstType::ObjectExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let mut props: Vec<(Cow<'a, str>, Span)> = Vec::new();
        let (span, is_class, class_name) = match node.kind() {
            AstKind::Class(c) if c.is_declaration() && is_es6_component(c) => {
                for el in &c.body.body {
                    let (key, computed, is_static) = match el {
                        // Abstract members are TS-only nodes upstream (not collected).
                        ClassElement::MethodDefinition(m)
                            if m.r#type == MethodDefinitionType::MethodDefinition =>
                        {
                            (&m.key, m.computed, m.r#static)
                        }
                        ClassElement::PropertyDefinition(p)
                            if p.r#type == PropertyDefinitionType::PropertyDefinition =>
                        {
                            (&p.key, p.computed, p.r#static)
                        }
                        _ => continue,
                    };
                    if !is_static && let Some(n) = key_name(key, computed) {
                        props.push((n, key.span()));
                    }
                }
                (c.span, true, c.id.as_ref().map(|i| i.name.as_str()))
            }
            AstKind::ObjectExpression(o) if is_es5_component(ctx, node.id()) => {
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p
                        && let Some(n) = key_name(&p.key, p.computed)
                    {
                        props.push((n, p.key.span()));
                    }
                }
                (o.span, false, None)
            }
            _ => return,
        };

        // Walk the subtree (node ids are allocated in pre-order).
        let nodes = ctx.nodes();
        let mut used: FxHashSet<Cow<'a, str>> = FxHashSet::default();
        let mut static_until = 0u32;
        let mut i = node.id().index() + 1;
        while i < nodes.len() {
            let n = nodes.get_node(NodeId::from_usize(i));
            i += 1;
            let kind = n.kind();
            let sp = kind.span();
            if sp.start >= span.end {
                break;
            }
            if sp.start < static_until {
                continue;
            }
            match kind {
                AstKind::MethodDefinition(m) if m.r#static => {
                    static_until = m.span.end;
                    continue;
                }
                AstKind::PropertyDefinition(p) if p.r#static => {
                    static_until = p.span.end;
                    continue;
                }
                AstKind::VariableDeclarator(v)
                    if v.init
                        .as_ref()
                        .is_some_and(|i| matches!(strip_ts(i), Expression::ThisExpression(_))) =>
                {
                    if let BindingPattern::ObjectPattern(p) = &v.id {
                        for bp in &p.properties {
                            if let Some(n) = key_name(&bp.key, bp.computed) {
                                used.insert(n);
                            }
                        }
                    }
                }
                _ => {}
            }
            if let Some(name) = this_member_name(&kind) {
                let is_assigned = matches!(ctx.parent_kind(n.id()), AstKind::AssignmentExpression(a) if a.left.span() == sp);
                if is_assigned {
                    if let AstKind::StaticMemberExpression(m) = kind {
                        props.push((name, m.property.span));
                    } else if let AstKind::ComputedMemberExpression(m) = kind {
                        props.push((name, m.expression.span()));
                    }
                } else {
                    used.insert(name);
                }
            }
        }

        for (name, sp) in props {
            if used.contains(&name)
                || LIFECYCLE.contains(&name.as_ref())
                || (is_class && name == "state")
                || (!is_class
                    && matches!(
                        name.as_ref(),
                        "getInitialState" | "getDefaultProps" | "mixins"
                    ))
            {
                continue;
            }
            let msg = match class_name {
                Some(c) => format!(
                    "Remove this property or method or refactor \"{c}\", as \"{name}\" is not used inside component body"
                ),
                None => format!(
                    "Remove this property or method or refactor this component, as \"{name}\" is not used inside component body"
                ),
            };
            report_react(ctx, sp, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_methods() {
        check(
            "S6441",
            "a.js",
            r#"
import React from 'react';
class Foo2 extends React.Component {
  handleClick() {} // Noncompliant {{Remove this property or method or refactor "Foo2", as "handleClick" is not used inside component body}}
  used() {}
  alsoUsed = () => {};
  destructured() {}
  static helper() { this.notCounted(); }
  state = {};
  componentDidMount() {
    this.timer = 1; // Noncompliant
    this.used();
    const { destructured } = this;
  }
  render() {
    return <div onClick={this.alsoUsed} />;
  }
}
class NotAComponent {
  unused() {}
}
const C = createReactClass({
  getInitialState() { return {}; },
  foo() {}, // Noncompliant {{Remove this property or method or refactor this component, as "foo" is not used inside component body}}
  bar() {},
  render() { this.bar(); return null; },
});
"#,
        );
    }
}
