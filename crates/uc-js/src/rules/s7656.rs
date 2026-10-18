//! Angular Pipes should implement PipeTransform interface
use crate::helpers::batch_ts::{angular_decorator, implements_name};
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
        let Some((_, decorator)) = angular_decorator(class, &["Pipe"]) else {
            return;
        };
        if !implements_name(class, "PipeTransform") {
            ctx.report(
                decorator.span,
                "Pipes should implement `PipeTransform` interface",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn pipe_transform() {
        check(
            "S7656",
            "a.ts",
            r#"
@Pipe({ name: 'a' }) // Noncompliant {{Pipes should implement `PipeTransform` interface}}
class A {
  transform(v: string) { return v; }
}
@Pipe({ name: 'b' })
class B implements PipeTransform {
  transform(v: string) { return v; }
}
class C {}
"#,
        );
    }
}
