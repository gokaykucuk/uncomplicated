//! The "outputs" metadata property should not be used in Angular components and directives
use crate::helpers::batch_ts::check_metadata_property;
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
        check_metadata_property(
            ctx,
            class,
            "outputs",
            "Use `@Output` rather than the `outputs` metadata property",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn outputs_metadata() {
        check(
            "S7654",
            "a.ts",
            r#"
@Directive({
  selector: 'a',
  outputs: ['change'], // Noncompliant {{Use `@Output` rather than the `outputs` metadata property}}
})
class A {}
@Component({ selector: 'b' })
class B {}
"#,
        );
    }
}
