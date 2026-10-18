//! Standard outputs should not be used directly to log anything
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

const DEFAULT_ALLOW: &[&str] = &[
    "assert",
    "clear",
    "count",
    "group",
    "groupCollapsed",
    "groupEnd",
    "info",
    "table",
    "time",
    "timeEnd",
    "trace",
];

pub struct Rule {
    allow: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let allow = p
            .list("allow", DEFAULT_ALLOW)
            .into_iter()
            .map(|s| {
                s.trim_matches(|c: char| {
                    c == '[' || c == ']' || c == '\'' || c == '"' || c.is_whitespace()
                })
                .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        Self { allow }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (object, prop, span) = match node.kind() {
            AstKind::StaticMemberExpression(m) => {
                (&m.object, Some(m.property.name.as_str()), m.span)
            }
            AstKind::ComputedMemberExpression(m) => {
                (&m.object, string_value(&m.expression), m.span)
            }
            _ => return,
        };
        let Expression::Identifier(id) = object else {
            return;
        };
        if id.name != "console" || ctx.symbol_of(id).is_some() {
            return;
        }
        if prop.is_some_and(|p| self.allow.iter().any(|a| a == p)) {
            return;
        }
        if !eslint_disabled(ctx, span.start, "no-console") {
            ctx.report(span, "Unexpected console statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn console() {
        check(
            "S106",
            "a.js",
            r#"
console.log("x");    // Noncompliant {{Unexpected console statement.}}
console.error("x");  // Noncompliant
console["warn"](1);  // Noncompliant
console.info("x");
console.table([]);
function f(console) { console.log(1); }
"#,
        );
        check(
            "S106",
            "c.js",
            "/* eslint no-console: 0 */\nconsole.log(1);\n",
        );
        check_with(
            "S106",
            "b.js",
            serde_json::json!({"allow": "log"}),
            "console.log(1);\nconsole.info(1); // Noncompliant\n",
        );
    }
}
