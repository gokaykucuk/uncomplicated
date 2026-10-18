//! Replacement strings should reference existing regular expression groups
use oxc_regular_expression::ast::Term;

use crate::helpers::batch_regex::{
    GroupRef, Node, group_references, is_string_replace_call, walk, with_expr_regex,
};
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
        if !is_string_replace_call(ctx, call) {
            return;
        }
        let (Some(pattern), Some(substr)) = (
            call.arguments[0].as_expression(),
            call.arguments[1].as_expression(),
        ) else {
            return;
        };
        let Expression::StringLiteral(s) = strip_parens(substr) else {
            return;
        };
        let refs = group_references(s.value.as_str());
        if refs.is_empty() {
            return;
        }
        with_expr_regex(ctx, pattern, |re| {
            let mut count = 0usize;
            let mut names: Vec<&str> = Vec::new();
            walk(re.pattern, &mut |n, enter| {
                if let (Node::Term(Term::CapturingGroup(g)), true) = (n, enter) {
                    count += 1;
                    if let Some(name) = &g.name {
                        names.push(name.as_str());
                    }
                }
            });
            let invalid: Vec<&str> = refs
                .iter()
                .filter(|r| match r {
                    GroupRef::Index { index, .. } => !index.is_some_and(|i| i >= 1 && i <= count),
                    GroupRef::Name { name, .. } => !names.contains(name),
                })
                .map(GroupRef::raw)
                .collect();
            if !invalid.is_empty() {
                let s = if invalid.len() > 1 { "s" } else { "" };
                ctx.report(
                    substr.span(),
                    format!("Referencing non-existing group{s}: {}.", invalid.join(", ")),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn existing_groups() {
        check(
            "S6328",
            "a.js",
            r#"
const str = "John Smith";
str.replace(/(\w+)\s(\w+)/, "$1, $0 $3");   // Noncompliant {{Referencing non-existing groups: $0, $3.}}
str.replace(/(?<first>\w+)/, "$<last>");     // Noncompliant {{Referencing non-existing group: $<last>.}}
str.replace(new RegExp("(a)"), "$2");        // Noncompliant
str.replace(/(\w+)\s(\w+)/, "$2, $1");
str.replace(/(?<first>\w+)/, "$<first>");
str.replace(/a/, "$$");
foo.replace(/a/, "$1");
str.replace(re, "$1");
"#,
        );
    }
}
