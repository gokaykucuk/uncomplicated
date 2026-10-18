//! Constructors should not return values
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ReturnStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ReturnStatement(r) = node.kind() else {
            return;
        };
        if r.argument.is_none() {
            return;
        }
        let Some(func) = helpers::batch_smell3::enclosing_function(ctx, node.id()) else {
            return;
        };
        if matches!(ctx.parent_kind(func.id()), AstKind::MethodDefinition(m) if m.kind == MethodDefinitionKind::Constructor)
        {
            ctx.report(r.span, "Unexpected return statement in constructor.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn constructor_return() {
        check(
            "S6635",
            "a.js",
            r#"
class A {
  constructor(a) {
    if (!a) {
      return;
    }
    this.a = a;
    return a;              // Noncompliant {{Unexpected return statement in constructor.}}
  }
  method() {
    return 1;
  }
}
class B {
  constructor() {
    this.f = () => { return 1; };
  }
}
"#,
        );
    }
}
