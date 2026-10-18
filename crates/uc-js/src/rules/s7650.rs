//! Components and directives should not use the "inputs" metadata property
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
            "inputs",
            "Use `@Input` rather than the `inputs` metadata property",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inputs_metadata() {
        check(
            "S7650",
            "a.ts",
            r#"
@Component({
  selector: 'a',
  inputs: ['id'], // Noncompliant {{Use `@Input` rather than the `inputs` metadata property}}
})
class A {}
@Directive({
  selector: 'b',
  hostDirectives: [{ directive: X, inputs: ['id'] }],
})
class B {}
"#,
        );
    }
}
