//! React state setter function should not be called with its matching state variable
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if call.arguments.len() != 1 {
            return;
        }
        let Some(Expression::Identifier(arg)) = call.arguments[0].as_expression() else {
            return;
        };
        let (Some(setter), Some(value)) = (ctx.symbol_of(callee), ctx.symbol_of(arg)) else {
            return;
        };
        let AstKind::VariableDeclarator(v) = ctx.symbol_decl(setter).kind() else {
            return;
        };
        let BindingPattern::ArrayPattern(p) = &v.id else {
            return;
        };
        if p.elements.len() != 2 || p.rest.is_some() {
            return;
        }
        let (
            Some(BindingPattern::BindingIdentifier(val)),
            Some(BindingPattern::BindingIdentifier(set)),
        ) = (&p.elements[0], &p.elements[1])
        else {
            return;
        };
        if set.symbol_id.get() != Some(setter) || val.symbol_id.get() != Some(value) {
            return;
        }
        let Some(Expression::CallExpression(init)) = v.init.as_ref() else {
            return;
        };
        let is_use_state = match &init.callee {
            Expression::Identifier(id) => id.name == "useState",
            Expression::StaticMemberExpression(m) => {
                m.property.name == "useState" && matches!(m.object, Expression::Identifier(_))
            }
            _ => false,
        };
        if !is_use_state
            || !fqn(ctx, &init.callee).is_some_and(|f| f.split('.').next() == Some("react"))
        {
            return;
        }
        report_react(
            ctx,
            call.span,
            "Change the argument of this setter to not use its matching state variable",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_set_state() {
        check(
            "S6443",
            "a.ts",
            r#"
import react, { useState } from 'react';

function ShowLanguage1() {
  const [language, setLanguage] = react.useState("fr-FR");
  setLanguage(navigator.language);
  setLanguage(language); // Noncompliant {{Change the argument of this setter to not use its matching state variable}}
}

function ShowLanguage2() {
  const [language, setLanguage] = useState("fr-FR");
  setLanguage(language); // Noncompliant
}

function ShowLanguage3() {
  const [language1, setLanguage1] = useState("fr-FR");
  const [language2, setLanguage2] = useState("France");
  setLanguage1(language2);
  setLanguage2(language1);
}

function NotReact() {
  const [a, setA] = other.useState(1);
  setA(a);
}
"#,
        );
    }
}
