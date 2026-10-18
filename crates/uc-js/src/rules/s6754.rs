//! The return value of "useState" should be destructured and named symmetrically
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    allow_destructured_state: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_destructured_state: p.bool("allowDestructuredState", true),
        }
    }
}

const MSG: &str = "useState call is not destructured into value + setter pair";

fn import_of<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'_>,
) -> Option<(AstKind<'a>, &'a str)> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    let AstKind::ImportDeclaration(d) = ctx.parent_kind(decl.id()) else {
        return None;
    };
    Some((decl.kind(), d.source.value.as_str()))
}

/// `utils.isReactHookCall(node, ['useState'])`: needs an ES import from 'react'.
fn is_react_use_state(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match &call.callee {
        Expression::Identifier(id) => matches!(import_of(ctx, id),
            Some((AstKind::ImportSpecifier(s), "react")) if s.imported.name() == "useState"),
        Expression::StaticMemberExpression(m) => {
            m.property.name == "useState"
                && matches!(&m.object, Expression::Identifier(o)
                    if matches!(import_of(ctx, o), Some((AstKind::ImportDefaultSpecifier(_), "react"))))
        }
        _ => false,
    }
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Upstream symmetric-name check.
fn upstream_symmetric(value: &str, setter: &str) -> bool {
    let prefix_len = value.bytes().take_while(u8::is_ascii_lowercase).count();
    if prefix_len == 0 {
        return false;
    }
    let (prefix, suffix) = value.split_at(prefix_len);
    setter == format!("set{}{suffix}", upper_first(prefix))
        || setter == format!("set{}{suffix}", prefix.to_uppercase())
}

/// Sonar's additional accepted patterns.
fn sonar_symmetric(value: &str, setter: &str) -> bool {
    let Some(suffix) = setter.strip_prefix("set") else {
        return false;
    };
    suffix == value || (value.len() > 2 && value.strip_prefix("is") == Some(suffix))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_react_use_state(ctx, call) {
            return;
        }
        let parent = estree_parent(ctx, node.id());
        let pattern = match parent.kind() {
            AstKind::ReturnStatement(_) => return,
            AstKind::VariableDeclarator(v) => match &v.id {
                BindingPattern::ArrayPattern(p) => p,
                _ => {
                    ctx.report(call.span, MSG);
                    return;
                }
            },
            _ => {
                ctx.report(call.span, MSG);
                return;
            }
        };
        let elements = &pattern.elements;
        let count = elements.len() + usize::from(pattern.rest.is_some());
        let is_destructuring = |p: Option<&BindingPattern<'_>>| {
            matches!(
                p,
                Some(BindingPattern::ArrayPattern(_) | BindingPattern::ObjectPattern(_))
            )
        };
        let value = elements.first().and_then(Option::as_ref);
        let setter = elements.get(1).and_then(Option::as_ref);
        if self.allow_destructured_state && is_destructuring(value) && !is_destructuring(setter) {
            return;
        }
        let name = |p: Option<&BindingPattern<'a>>| match p {
            Some(BindingPattern::BindingIdentifier(id)) => Some(id.name.as_str()),
            _ => None,
        };
        if count == 1 {
            return; // Sonar: `const [value] = useState()` is fine.
        }
        let (vn, sn) = (name(value), name(setter));
        let ok = count == 2
            && match (vn, sn) {
                (Some(v), Some(s)) => upstream_symmetric(v, s) || sonar_symmetric(v, s),
                _ => false,
            };
        if !ok {
            ctx.report(pattern.span, MSG);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn use_state_naming() {
        check(
            "S6754",
            "a.js",
            r#"
import React, { useState } from 'react';
function useFoo() {
  const [foo] = useState();
  const [foo1, setFoo1] = useState();
  const [Foo, setFoo] = useState();
  const [isReady, setReady] = useState(false);
  const [isOpen, setIsOpen] = useState(false);
  const [url, setURL] = useState();
  const [{ a }, setA] = useState({});
  const [x, bar] = useState(); // Noncompliant {{useState call is not destructured into value + setter pair}}
  const [isReady2, updateReady] = useState(false); // Noncompliant
  const [is, set] = useState(false); // Noncompliant
  const getterAndSetter = useState(); // Noncompliant
  const [y, setY] = React.useState();
  const [z, other] = React.useState(); // Noncompliant
  [client.searchState, client.setSearchState] = useState(baseState); // Noncompliant
  return useState();
}
"#,
        );
    }
}
