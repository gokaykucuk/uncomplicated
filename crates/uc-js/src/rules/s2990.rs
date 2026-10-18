//! The global "this" object should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        let object = match node.kind() {
            AstKind::StaticMemberExpression(m) => &m.object,
            AstKind::ComputedMemberExpression(m) => &m.object,
            _ => return,
        };
        let Expression::ThisExpression(this) = object else {
            return;
        };
        let in_function_or_class = ctx.nodes().ancestors(node.id()).any(|a| {
            matches!(
                a.kind(),
                AstKind::Function(_)
                    | AstKind::ArrowFunctionExpression(_)
                    | AstKind::Class(_)
                    | AstKind::TSModuleBlock(_)
            )
        });
        if !in_function_or_class {
            ctx.report(this.span, "Remove the use of \"this\".");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn global_this() {
        check(
            "S2990",
            "a.js",
            r#"
this.foo = 1; // Noncompliant {{Remove the use of "this".}}
console.log(this["bar"]); // Noncompliant
if (x) { this.baz(); } // Noncompliant
function f() { this.a = 1; }
const g = () => this.b;
class C { x = this.y; m() { return this.z; } }
var o = { m() { return this.q; } };
"#,
        );
    }
}
