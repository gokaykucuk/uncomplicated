//! Unnecessary type constraints should be removed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSTypeParameter]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSTypeParameter(p) = node.kind() else {
            return;
        };
        let constraint = match &p.constraint {
            Some(TSType::TSAnyKeyword(_)) => "any",
            Some(TSType::TSUnknownKeyword(_)) => "unknown",
            _ => return,
        };
        ctx.report(
            p.span,
            format!(
                "Constraining the generic type `{}` to `{constraint}` does nothing and is unnecessary.",
                p.name.name
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn constraints() {
        check(
            "S6569",
            "a.ts",
            r#"
function f<T extends any>() {} // Noncompliant {{Constraining the generic type `T` to `any` does nothing and is unnecessary.}}
interface I<T extends unknown> {} // Noncompliant {{Constraining the generic type `T` to `unknown` does nothing and is unnecessary.}}
const g = <T extends unknown>() => {}; // Noncompliant
class C<T extends string> {}
type A<T> = T;
"#,
        );
    }
}
