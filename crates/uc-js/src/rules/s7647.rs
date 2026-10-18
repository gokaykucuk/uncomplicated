//! Empty Angular lifecycle methods should be removed
use crate::helpers::batch_ts::{ANGULAR_CLASS_DECORATORS, ANGULAR_LIFECYCLE, angular_decorator};
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
        if angular_decorator(class, ANGULAR_CLASS_DECORATORS).is_none() {
            return;
        }
        for el in &class.body.body {
            let ClassElement::MethodDefinition(m) = el else {
                continue;
            };
            let PropertyKey::StaticIdentifier(id) = &m.key else {
                continue;
            };
            if m.computed || !ANGULAR_LIFECYCLE.iter().any(|(n, _)| *n == id.name) {
                continue;
            }
            if m.value
                .body
                .as_ref()
                .is_some_and(|b| b.statements.is_empty())
            {
                ctx.report(m.span, "Lifecycle methods should not be empty");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_lifecycle() {
        check(
            "S7647",
            "a.ts",
            r#"
@Component({ selector: 'x' })
class C {
  ngOnInit() {} // Noncompliant {{Lifecycle methods should not be empty}}
  ngOnDestroy() { // Noncompliant
    // only a comment
  }
  ngOnChanges() { this.x = 1; }
  other() {}
}
class Plain {
  ngOnInit() {}
}
"#,
        );
    }
}
