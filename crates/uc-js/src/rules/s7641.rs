//! Angular lifecycle methods should be used in the correct context
use crate::helpers::batch_ts::{ANGULAR_CLASS_DECORATORS, ANGULAR_LIFECYCLE, angular_decorator};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Lifecycle methods Angular invokes for classes with the given decorator.
fn allowed(decorator: &str, method: &str) -> bool {
    match decorator {
        "Component" | "Directive" => method != "ngDoBootstrap",
        "Injectable" | "Pipe" => method == "ngOnDestroy",
        "NgModule" => method == "ngDoBootstrap",
        _ => true,
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
        let Some((decorator, _)) = angular_decorator(class, ANGULAR_CLASS_DECORATORS) else {
            return;
        };
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
            if !allowed(decorator, &id.name) {
                ctx.report(
                    id.span,
                    format!(
                        "Angular will not invoke the `{}` lifecycle method within `@{decorator}()` classes",
                        id.name
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn contextual_lifecycle() {
        check(
            "S7641",
            "a.ts",
            r#"
@Injectable()
class S {
  ngOnInit() {} // Noncompliant {{Angular will not invoke the `ngOnInit` lifecycle method within `@Injectable()` classes}}
  ngOnDestroy() {}
}
@Pipe({ name: 'p' })
class P {
  ngOnChanges() {} // Noncompliant
}
@Component({})
class C {
  ngOnInit() {}
  ngDoBootstrap() {} // Noncompliant
}
@NgModule({})
class M {
  ngDoBootstrap() {}
  ngOnInit() {} // Noncompliant
}
class Plain {
  ngOnInit() {}
}
"#,
        );
    }
}
