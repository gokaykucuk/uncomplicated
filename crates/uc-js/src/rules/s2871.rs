//! "Array.prototype.sort()" and "Array.prototype.toSorted()" should use a compare function
use crate::helpers::batch_bug1::{array_elem, is_html, parent_skip_parens_only};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `JSON.stringify(a.sort()) === ...` / `a.sort().join() === ...`: order normalization for comparison.
fn is_normalization_comparison(ctx: &Ctx<'_, '_>, call_node: &AstNode<'_>) -> bool {
    let parent = parent_skip_parens_only(ctx, call_node.id());
    let outer = match parent.kind() {
        AstKind::CallExpression(c) => {
            let is_stringify = matches!(strip_parens(&c.callee), Expression::StaticMemberExpression(m)
                if m.property.name == "stringify" && matches!(strip_parens(&m.object), Expression::Identifier(j) if j.name == "JSON"));
            if !is_stringify || c.arguments.len() != 1 {
                return false;
            }
            parent
        }
        AstKind::StaticMemberExpression(m) if m.property.name == "join" => {
            let call = parent_skip_parens_only(ctx, parent.id());
            if !matches!(call.kind(), AstKind::CallExpression(_)) {
                return false;
            }
            call
        }
        _ => return false,
    };
    matches!(parent_skip_parens_only(ctx, outer.id()).kind(), AstKind::BinaryExpression(b) if b.operator.is_equality())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !call.arguments.is_empty() {
            return;
        }
        let (object, prop_span) = match &call.callee {
            Expression::StaticMemberExpression(m)
                if matches!(m.property.name.as_str(), "sort" | "toSorted") =>
            {
                (&m.object, m.property.span)
            }
            Expression::ComputedMemberExpression(m)
                if m.static_property_name()
                    .is_some_and(|n| n == "sort" || n == "toSorted") =>
            {
                (&m.object, m.expression.span())
            }
            _ => return,
        };
        if is_html(ctx) {
            return;
        }
        let Some(elem) = array_elem(ctx, object, call.span.start) else {
            return;
        };
        // `Ty::Any` is TypeScript's `any[]`: still an array, of unknown elements.
        let is_string = match &elem {
            Ty::Any => false,
            t if t.is_unknown() => return,
            t => t.is_string(),
        };
        if is_normalization_comparison(ctx, node) {
            return;
        }
        let msg = if is_string {
            "Provide a compare function that depends on \"String.localeCompare\", to reliably sort elements alphabetically."
        } else {
            "Provide a compare function to avoid sorting elements alphabetically."
        };
        ctx.report(prop_span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn sort_without_compare() {
        check(
            "S2871",
            "a.ts",
            r#"
const nums = [3, 1, 2];
nums.sort(); // Noncompliant {{Provide a compare function to avoid sorting elements alphabetically.}}
const strs: string[] = ["b", "a"];
strs.sort(); // Noncompliant {{Provide a compare function that depends on "String.localeCompare", to reliably sort elements alphabetically.}}
strs.toSorted(); // Noncompliant
Object.keys(obj).sort(); // Noncompliant
function f(a: Array<number>) { return a.sort(); } // Noncompliant
nums.sort((a, b) => a - b);
strs.sort((a, b) => a.localeCompare(b));
unknown.sort();
const typed = new Int32Array(3);
typed.sort();
var names = [];
for (var key in obj) { names.push(key); }
names.sort(); // Noncompliant {{Provide a compare function that depends on "String.localeCompare", to reliably sort elements alphabetically.}}
var roots = [];
solve(roots);
roots.sort(); // Noncompliant {{Provide a compare function to avoid sorting elements alphabetically.}}
var mixed = [];
mixed.push(foo());
mixed.sort();
const doubled = nums.map(x => x * 2);
doubled.sort(); // Noncompliant
"a.b".split(".").sort(); // Noncompliant
if (JSON.stringify(strs.sort()) === JSON.stringify(other)) {}
if (strs.sort().join() === other.join()) {}
"#,
        );
    }
}
