//! React legacy lifecycle methods should not be used
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    check_aliases: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            check_aliases: p.bool("checkAliases", true),
        }
    }
}

const UNSAFE: &[&str] = &[
    "UNSAFE_componentWillMount",
    "UNSAFE_componentWillReceiveProps",
    "UNSAFE_componentWillUpdate",
];
const ALIASES: &[&str] = &[
    "componentWillMount",
    "componentWillReceiveProps",
    "componentWillUpdate",
];

impl Rule {
    fn report<'a>(&self, ctx: &Ctx<'a, '_>, members: impl Iterator<Item = (&'a str, Span)>) {
        let mut seen: Vec<&str> = Vec::new();
        for (name, span) in members {
            if seen.contains(&name) {
                continue;
            }
            if UNSAFE.contains(&name) || (self.check_aliases && ALIASES.contains(&name)) {
                seen.push(name);
                ctx.report(
                    span,
                    format!("{name} is unsafe for use in async rendering."),
                );
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class, AstType::ObjectExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Class(c) if is_es6_component(c) => self.report(
                ctx,
                c.body
                    .body
                    .iter()
                    .filter_map(|el| Some((class_member_name(el)?, el.property_key()?.span()))),
            ),
            AstKind::ObjectExpression(o) if is_es5_component(ctx, node.id()) => self.report(
                ctx,
                o.properties.iter().filter_map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(p) if !p.computed => match &p.key {
                        PropertyKey::StaticIdentifier(id) => Some((id.name.as_str(), p.key.span())),
                        _ => None,
                    },
                    _ => None,
                }),
            ),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsafe_lifecycle() {
        check(
            "S6791",
            "a.jsx",
            r#"
import React from 'react';
class A extends React.Component {
  UNSAFE_componentWillMount() {} // Noncompliant {{UNSAFE_componentWillMount is unsafe for use in async rendering.}}
  componentWillReceiveProps() {} // Noncompliant {{componentWillReceiveProps is unsafe for use in async rendering.}}
  componentDidUpdate() {}
  render() {}
}
class B {
  componentWillMount() {}
}
const C = createReactClass({
  componentWillUpdate() {}, // Noncompliant
});
"#,
        );
    }
}
