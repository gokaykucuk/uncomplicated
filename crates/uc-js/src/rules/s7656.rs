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
        if class.r#type != ClassType::ClassDeclaration
            || angular_decorator(class, &["Pipe"]).is_none()
            || implements_name(class, "PipeTransform")
        {
            return;
        }
        let span = class.id.as_ref().map_or(class.span, |id| id.span);
        ctx.report(span, "Pipes should implement `PipeTransform` interface");
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
@Pipe({ name: 'a' })
class A { // Noncompliant {{Pipes should implement `PipeTransform` interface}}
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
