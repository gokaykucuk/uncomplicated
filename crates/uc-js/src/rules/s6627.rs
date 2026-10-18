//! Users should not use internal APIs
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Do not use internal APIs of your dependencies";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::ImportDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) => {
                if matches!(&c.callee, Expression::Identifier(id) if id.name == "require")
                    && c.arguments.len() == 1
                    && let Some(Expression::StringLiteral(s)) = c.arguments[0].as_expression()
                    && s.value.contains("node_modules")
                {
                    ctx.report(c.span, MESSAGE);
                }
            }
            AstKind::ImportDeclaration(i) if i.source.value.contains("node_modules") => {
                ctx.report(i.span, MESSAGE);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn internal_api() {
        check(
            "S6627",
            "a.js",
            r#"
import { a } from 'pkg/node_modules/dep/lib';   // Noncompliant {{Do not use internal APIs of your dependencies}}
import { b } from 'pkg';
const c = require('../node_modules/x/y');        // Noncompliant
const d = require('x');
"#,
        );
    }
}
