//! React's "findDOMNode" should not be used
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
        let AstKind::CallExpression(c) = node.kind() else {
            return;
        };
        let hit = match &c.callee {
            Expression::Identifier(id) => id.name == "findDOMNode",
            Expression::StaticMemberExpression(m) => m.property.name == "findDOMNode",
            _ => false,
        };
        if hit {
            report_react(
                ctx,
                c.callee.span(),
                "Do not use findDOMNode. It doesn't work with function components and is deprecated in StrictMode.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn find_dom_node() {
        check(
            "S6788",
            "a.jsx",
            r#"
import React from "react";
class MyComponent extends Component {
  componentDidMount() {
    findDOMNode(this).scrollIntoView(); // Noncompliant {{Do not use findDOMNode. It doesn't work with function components and is deprecated in StrictMode.}}
    ReactDOM.findDOMNode(this); // Noncompliant
    this.ref.current.focus();
  }
}
"#,
        );
    }

    #[test]
    fn non_react_files() {
        check(
            "S6788",
            "a.jsx",
            r#"
import { h } from 'vue';
findDOMNode(this);
"#,
        );
        check("S6788", "b.js", "findDOMNode(this);\n");
    }
}
