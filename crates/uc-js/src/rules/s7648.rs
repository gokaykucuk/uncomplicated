//! Components, Directives, and Pipes should use standalone architecture
use crate::helpers::batch_ts::{angular_decorator, decorator_metadata, object_property};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        let Some((_, decorator)) = angular_decorator(class, &["Component", "Directive", "Pipe"])
        else {
            return;
        };
        let Some(meta) = decorator_metadata(decorator) else {
            return;
        };
        let Some(standalone) = object_property(meta, "standalone") else {
            return;
        };
        if matches!(strip_parens(&standalone.value), Expression::BooleanLiteral(b) if !b.value) {
            ctx.report(
                decorator.span,
                "Components, Directives and Pipes should not opt out of standalone",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn standalone() {
        check(
            "S7648",
            "a.ts",
            r#"
@Component({ // Noncompliant {{Components, Directives and Pipes should not opt out of standalone}}
  selector: 'a',
  standalone: false,
})
class A {}
@Directive({ selector: 'b', standalone: true })
class B {}
@Pipe({ name: 'c' })
class C {}
"#,
        );
    }
}
