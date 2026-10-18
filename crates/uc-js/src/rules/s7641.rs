//! Angular lifecycle methods should be used in the correct context
use crate::helpers::batch_ts::{
    ANGULAR_CLASS_DECORATORS, ANGULAR_LIFECYCLE, angular_decorator, angular_method_name,
};
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
        "Injectable" | "Pipe" | "Service" => method == "ngOnDestroy",
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
        if class.r#type != ClassType::ClassDeclaration {
            return;
        }
        let Some((decorator, _)) = angular_decorator(class, ANGULAR_CLASS_DECORATORS) else {
            return;
        };
        for el in &class.body.body {
            let ClassElement::MethodDefinition(m) = el else {
                continue;
            };
            let Some((name, span)) = angular_method_name(m) else {
                continue;
            };
            if !ANGULAR_LIFECYCLE.iter().any(|(n, _)| *n == name) {
                continue;
            }
            if !allowed(decorator, name) {
                ctx.report(
                    span,
                    format!(
                        "Angular will not invoke the `{name}` lifecycle method within `@{decorator}()` classes"
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
