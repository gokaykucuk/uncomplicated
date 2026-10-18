//! Regular expressions used in Testing Library queries should not have the global flag
//!
//! `testing-library/no-global-regexp-flag-in-query`. SonarJS only enables it for projects that
//! depend on Testing Library; we require the file to import it.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Avoid using the global RegExp flag (/g) in queries";

/// `(get|query|find)(All)?By(LabelText|PlaceholderText|Text|AltText|Title|DisplayValue|Role|TestId)`
fn is_query(name: &str) -> bool {
    let rest = ["get", "query", "find"]
        .iter()
        .find_map(|p| name.strip_prefix(p));
    let Some(rest) = rest else { return false };
    let rest = rest.strip_prefix("All").unwrap_or(rest);
    let Some(kind) = rest.strip_prefix("By") else {
        return false;
    };
    matches!(
        kind,
        "LabelText"
            | "PlaceholderText"
            | "Text"
            | "AltText"
            | "Title"
            | "DisplayValue"
            | "Role"
            | "TestId"
    )
}

fn imports_testing_library(ctx: &Ctx<'_, '_>) -> bool {
    let AstKind::Program(p) = ctx.node(NodeId::ROOT).kind() else {
        return false;
    };
    p.body.iter().any(|s| {
        matches!(s, Statement::ImportDeclaration(i) if {
            let src = i.source.value.as_str();
            src.starts_with("@testing-library/") || src == "@storybook/test" || src == "storybook/test"
        })
    })
}

fn global_regex<'a>(e: &'a Expression<'a>) -> Option<Span> {
    match strip_parens(e) {
        Expression::RegExpLiteral(l) if l.regex.flags.contains(RegExpFlags::G) => Some(l.span),
        _ => None,
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
        let name = match &call.callee {
            Expression::Identifier(id) => id.name.as_str(),
            Expression::StaticMemberExpression(m) => m.property.name.as_str(),
            _ => return,
        };
        if !is_query(name) || !imports_testing_library(ctx) {
            return;
        }
        let first = call.arguments.first().and_then(Argument::as_expression);
        if let Some(span) = first.and_then(global_regex) {
            ctx.report(span, MESSAGE);
            return;
        }
        if let Some(Expression::Identifier(id)) = first.map(strip_parens)
            && let Some(sym) = ctx.symbol_of(id)
            && let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind()
            && let Some(span) = v.init.as_ref().and_then(global_regex)
        {
            ctx.report(span, MESSAGE);
        }
        if let Some(Expression::ObjectExpression(o)) =
            call.arguments.get(1).and_then(Argument::as_expression)
        {
            let name_value = o.properties.iter().find_map(|p| match p {
                ObjectPropertyKind::ObjectProperty(p)
                    if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "name") =>
                {
                    Some(&p.value)
                }
                _ => None,
            });
            if let Some(span) = name_value.and_then(global_regex) {
                ctx.report(span, MESSAGE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn global_flag_in_query() {
        check(
            "S8981",
            "a.test.js",
            r#"
import { screen } from '@testing-library/react';
screen.getByText(/hello/gi);                        // Noncompliant {{Avoid using the global RegExp flag (/g) in queries}}
screen.getByRole('button', { name: /submit/g });    // Noncompliant
const re = /foo/g;                                  // Noncompliant
screen.getByText(re);
screen.queryAllByText(/hello/g);                    // Noncompliant
await screen.findByText(/hello/g);                  // Noncompliant
screen.getByText(/hello/i);
screen.getByRole('button', { name: /submit/i });
someNonQueryFn(/foo/g);
"#,
        );
        check(
            "S8981",
            "b.test.js",
            r#"
screen.getByText(/hello/g);
"#,
        );
    }
}
