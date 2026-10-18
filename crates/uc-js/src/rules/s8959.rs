//! UI test debug commands should not be committed to version control
use crate::helpers::batch_tests::{chain_starts_with_cy, fqn, is_test_related_file};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TL_MODULES: &[&str] = &["dom", "react", "vue", "angular", "svelte", "preact"];
const TL_DEBUG_COMMANDS: &[&str] = &[
    ".screen.debug",
    ".screen.logTestingPlaygroundURL",
    ".render.debug",
    ".prettyDOM",
    ".logRoles",
];

fn is_testing_library_debug<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> bool {
    let Some(f) = fqn(ctx, &call.callee) else {
        return false;
    };
    let Some(rest) = f.strip_prefix("@testing-library.") else {
        return false;
    };
    TL_MODULES
        .iter()
        .any(|m| rest.starts_with(m) && rest.as_bytes().get(m.len()) == Some(&b'.'))
        && TL_DEBUG_COMMANDS.iter().any(|c| f.ends_with(c))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let span = match &call.callee {
            Expression::StaticMemberExpression(m) => {
                let debug = match m.property.name.as_str() {
                    "pause" => {
                        chain_starts_with_cy(&m.object)
                            || matches!(&m.object, Expression::Identifier(id) if id.name == "page")
                    }
                    "debug" => {
                        chain_starts_with_cy(&m.object) || is_testing_library_debug(ctx, call)
                    }
                    "logTestingPlaygroundURL" => is_testing_library_debug(ctx, call),
                    _ => false,
                };
                if !debug {
                    return;
                }
                m.property.span
            }
            Expression::Identifier(id) => {
                if !is_testing_library_debug(ctx, call) {
                    return;
                }
                id.span
            }
            _ => return,
        };
        if is_test_related_file(ctx.file.path) {
            ctx.report(span, "Remove this debug command from the test.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn debug_commands() {
        check(
            "S8959",
            "a.cy.js",
            r#"
cy.pause(); // Noncompliant {{Remove this debug command from the test.}}
cy.get('x').debug().click(); // Noncompliant
page.pause(); // Noncompliant
other.pause();
"#,
        );
        check(
            "S8959",
            "a.test.jsx",
            r#"
import { screen, prettyDOM, render } from '@testing-library/react';
screen.debug(); // Noncompliant
screen.logTestingPlaygroundURL(); // Noncompliant
prettyDOM(document.body); // Noncompliant
const view = render(<App />);
view.debug(); // Noncompliant
logger.debug('x');
"#,
        );
        check("S8959", "src/app.js", "cy.pause();");
    }
}
