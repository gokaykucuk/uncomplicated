//! Use "for...of" loops instead of "forEach" method calls
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
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        if callee.property.name != "forEach" || callee.optional || call.optional {
            return;
        }
        if matches!(
            dotted_path(&callee.object).as_deref(),
            Some("React.Children" | "Children")
        ) {
            return;
        }
        // Only iterable receivers can be rewritten as `for...of`. In plain JavaScript, an untyped
        // receiver is reported like SonarJS does for `any`.
        let t = type_of(ctx, &callee.object);
        let iterable = t.all(|t| matches!(t, Ty::Array | Ty::Tuple | Ty::Map | Ty::Set))
            || (t == Ty::Unknown
                && !ctx.is_ts()
                && !matches!(callee.object, Expression::ThisExpression(_))
                && call.arguments.len() == 1
                && matches!(
                    call.arguments[0],
                    Argument::FunctionExpression(_) | Argument::ArrowFunctionExpression(_)
                ));
        if iterable {
            ctx.report(
                callee.property.span,
                "Use `for…of` instead of `.forEach(…)`.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7728",
            "a.ts",
            r#"
[1, 2, 3].forEach(x => console.log(x)); // Noncompliant {{Use `for…of` instead of `.forEach(…)`.}}
const arr: number[] = [];
arr.forEach(x => console.log(x)); // Noncompliant
const set = new Set([1]);
set.forEach(x => console.log(x)); // Noncompliant
const map = new Map();
map.forEach((v, k) => console.log(k, v)); // Noncompliant
class Test { forEach(cb: () => void) {} }
new Test().forEach(() => true);
declare const collection: { forEach(cb: (x: string) => void): void };
collection.forEach(x => x);
unknown.forEach(x => x);
arr?.forEach(x => x);
"#,
        );
    }
}

#[cfg(test)]
mod tests_js {
    use crate::test_util::*;

    #[test]
    fn untyped_javascript() {
        check(
            "S7728",
            "a.js",
            r#"
items.forEach(item => use(item)); // Noncompliant
items.forEach(function (item) { use(item); }); // Noncompliant
this.forEach(item => use(item));
items.forEach(this.handle, this);
ts.forEach(list, item => item);
"#,
        );
    }
}
