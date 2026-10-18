//! Array indexes should be numeric
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
        &[AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::AssignmentExpression(assign) = node.kind() else {
            return;
        };
        let AssignmentTarget::ComputedMemberExpression(m) = &assign.left else {
            return;
        };
        if !is_string_like(ctx, &m.expression) {
            return;
        }
        if type_of(ctx, &m.object) != Ty::Array {
            return;
        }
        ctx.report(
            assign.span,
            "Make it an object if it must have named properties; otherwise, use a numeric index here.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn associative() {
        check(
            "S3579",
            "a.ts",
            r#"
const arr = [];
arr['name'] = 'bob';          // Noncompliant {{Make it an object if it must have named properties; otherwise, use a numeric index here.}}
const key: string = 'k';
arr[key] = 1;                 // Noncompliant
arr[0] = 1;
const obj = {};
obj['name'] = 'bob';
function f(a: any, s: string) { a[s] = 1; }
const n = 1;
arr[n] = 2;
"#,
        );
    }
}
