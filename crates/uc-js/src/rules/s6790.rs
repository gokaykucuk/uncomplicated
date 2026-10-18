//! String references should not be used
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
        &[AstType::JSXAttribute]
    }

    // `this.refs` is only reported upstream for React < 18.3; Sonar analyzes with the latest
    // React version, so only string refs are reported.
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if attr_name(a) != Some("ref") {
            return;
        }
        let is_string = match &a.value {
            Some(JSXAttributeValue::StringLiteral(_)) => true,
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                matches!(
                    c.expression.as_expression(),
                    Some(Expression::StringLiteral(_))
                )
            }
            _ => false,
        };
        if is_string {
            report_react(
                ctx,
                a.span,
                "Using string literals in ref attributes is deprecated.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn string_refs() {
        check(
            "S6790",
            "a.jsx",
            r#"
import React from "react";
const a = <div ref="hello" />; // Noncompliant {{Using string literals in ref attributes is deprecated.}}
const b = <div ref={"hello"} />; // Noncompliant
const c = <div ref={`hello`} />;
const d = <div ref={this.myRef} />;
"#,
        );
    }
}
