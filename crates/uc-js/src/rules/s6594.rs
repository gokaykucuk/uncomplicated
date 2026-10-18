//! "RegExp.exec()" should be preferred over "String.match()"
use crate::helpers::batch_regex::{lhs_symbol, with_expr_regex};
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
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        if m.property.name != "match" || call.arguments.len() != 1 {
            return;
        }
        let Some(arg) = call.arguments[0].as_expression() else {
            return;
        };
        if !type_of(ctx, &m.object).is_string() {
            return;
        }
        // Only when the regex is known not to be global: `match` with a global regex returns all
        // matches, which `exec` does not.
        let mut non_global = false;
        with_expr_regex(ctx, arg, |re| {
            non_global = !re.flags.contains(RegExpFlags::G)
        });
        if !non_global {
            return;
        }
        if let Some(sym) = lhs_symbol(ctx, node.id()) {
            let uses_length = ctx.scoping().get_resolved_references(sym).any(|r| {
                matches!(ctx.parent_kind(r.node_id()), AstKind::StaticMemberExpression(p) if p.property.name == "length")
            });
            if uses_length {
                return;
            }
        }
        ctx.report(m.property.span, "Use the \"RegExp.exec()\" method instead.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_exec() {
        check(
            "S6594",
            "a.js",
            r#"
const str = "abc";
str.match(/a/);                       // Noncompliant {{Use the "RegExp.exec()" method instead.}}
const m = str.match(/(b)/);           // Noncompliant
"foo".match(new RegExp("o"));         // Noncompliant
str.match(/a/g);
const n = str.match(/a/);
n.length;
foo.match(/a/);
str.match(re);
str.match(/a/, 1);
"#,
        );
    }
}
