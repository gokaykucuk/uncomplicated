//! React's useState hook should not be used directly in the render function or body of a component
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Sonar's `matchesReactComponentName(fn, 1)`: the function's own name, or the name it is
/// assigned to, starts with a non lower-case character.
fn has_component_name(ctx: &Ctx<'_, '_>, func: &AstNode<'_>) -> bool {
    let starts_ok = |n: &str| n.chars().next().is_some_and(|c| !c.is_ascii_lowercase());
    match func.kind() {
        AstKind::Function(f) if f.is_declaration() => {
            f.id.as_ref().is_some_and(|i| starts_ok(&i.name))
        }
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
            match estree_parent(ctx, func.id()).kind() {
                AstKind::VariableDeclarator(v) => match &v.id {
                    BindingPattern::BindingIdentifier(id) => starts_ok(&id.name),
                    _ => false,
                },
                AstKind::AssignmentExpression(a) => match &a.left {
                    AssignmentTarget::AssignmentTargetIdentifier(id) => starts_ok(&id.name),
                    AssignmentTarget::StaticMemberExpression(m) => starts_ok(&m.property.name),
                    _ => false,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

/// The outermost component function enclosing `id` (Sonar tracks the top-most one only).
fn outermost_component<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> Option<NodeId> {
    let mut found = None;
    for a in ancestors(ctx, id) {
        if is_function(&a.kind()) && has_component_name(ctx, a) {
            found = Some(a.id());
        }
    }
    found
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if call.arguments.len() != 1 {
            return;
        }
        let Some(sym) = ctx.symbol_of(callee) else {
            return;
        };
        // const [x, setX] = useState(init);
        let decl = ctx.symbol_decl(sym);
        let AstKind::VariableDeclarator(v) = decl.kind() else {
            return;
        };
        let BindingPattern::ArrayPattern(p) = &v.id else {
            return;
        };
        if p.elements.len() != 2 || p.rest.is_some() {
            return;
        }
        let (
            Some(BindingPattern::BindingIdentifier(_)),
            Some(BindingPattern::BindingIdentifier(setter)),
        ) = (&p.elements[0], &p.elements[1])
        else {
            return;
        };
        if setter.symbol_id.get() != Some(sym) {
            return;
        }
        let Some(Expression::CallExpression(init)) = v.init.as_ref() else {
            return;
        };
        if init.arguments.len() != 1 || !is_react_api(ctx, &init.callee, "useState") {
            return;
        }
        // Both the declaration and the call must be directly in the top-most component function.
        let Some(func) = enclosing_function(ctx, node.id()) else {
            return;
        };
        if enclosing_function(ctx, decl.id()).map(|f| f.id()) != Some(func.id()) {
            return;
        }
        if outermost_component(ctx, node.id()) != Some(func.id()) {
            return;
        }
        let in_if = ancestors(ctx, node.id())
            .take_while(|a| a.id() != func.id())
            .any(|a| matches!(a.kind(), AstKind::IfStatement(_)));
        if in_if {
            return;
        }
        report_react(
            ctx,
            callee.span,
            "Remove this state setter call, perhaps move it to an event handler or JSX attribute",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn setter_in_body() {
        check(
            "S6442",
            "a.js",
            r#"
import React, { useState } from 'react';

function notAReactComponent() {
  const [a, setA] = useState(0);
  setA(1);
}

function ShowLanguageInvalid() {
  const [language, setLanguage] = useState("fr-FR");
  setLanguage(navigator.language); // Noncompliant {{Remove this state setter call, perhaps move it to an event handler or JSX attribute}}
  return <button onClick={() => setLanguage("fr-FR")}>Fr</button>;
}

const Arrow = () => {
  const [language, setLanguage] = React.useState("fr-FR");
  setLanguage(navigator.language); // Noncompliant
  return null;
};

exports.MemberInvalid = () => {
  const [language, setLanguage] = useState("fr-FR");
  setLanguage(navigator.language); // Noncompliant
  return null;
};

function Multiple() {
  const [a, setA] = useState("a");
  const [b, setB] = useState("b");
  function setC(c) {}
  setA("A"); // Noncompliant
  setB("B"); // Noncompliant
  setC("C");
  return null;
}

function Nested() {
  const [language, setLanguage] = useState(navigator.language);
  if (language === 'fr') {
    setLanguage('en');
  }
  function process(setLanguage) {
    setLanguage('en');
  }
  return null;
}

function NoArg() {
  const [a, setA] = useState();
  setA(1);
}

const [count, setCount] = useState(0);
setCount(1);
"#,
        );
    }
}
