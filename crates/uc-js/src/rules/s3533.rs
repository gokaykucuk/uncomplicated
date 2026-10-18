//! "import" should be used to include external code
use crate::helpers::batch_smell3::is_function_value;
use crate::helpers::types::is_string_like;
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
        let name = callee.name.as_str();
        if name != "require" && name != "define" {
            return;
        }
        if node.scope_id() != ctx.scoping().root_scope_id() {
            return;
        }
        let args = &call.arguments;
        let arg_expr = |a: &'a Argument<'a>| a.as_expression();
        let common_js = name == "require"
            && args.len() == 1
            && arg_expr(&args[0]).is_some_and(|e| is_string_like(ctx, e));
        // AMD detection needs type information, which SonarJS doesn't have for HTML scripts.
        let amd = helpers::batch_smell3::has_type_services(ctx)
            && (args.len() == 2 || args.len() == 3)
            && args
                .last()
                .and_then(arg_expr)
                .is_some_and(|e| is_function_value(ctx, e));
        if common_js || amd {
            ctx.report(
                callee.span,
                format!("Use a standard \"import\" statement instead of \"{name}\"."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn require_define() {
        check(
            "S3533",
            "a.js",
            r#"
const fs = require('fs');          // Noncompliant {{Use a standard "import" statement instead of "require".}}
const p = require(`path`);         // Noncompliant
define(['a'], function (a) {});    // Noncompliant {{Use a standard "import" statement instead of "define".}}
require(['a', 'b'], (a, b) => {}); // Noncompliant
define('name', ['a'], function () {}); // Noncompliant
require(name);
require('a', 'b');
define(['a'], obj);
function f() { const x = require('x'); }
if (cond) { require('y'); }
other('z');
"#,
        );
    }
}
