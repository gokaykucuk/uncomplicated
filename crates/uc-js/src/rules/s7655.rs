//! Angular classes should implement lifecycle interfaces for their lifecycle methods
use crate::helpers::batch_ts::{implements_name, lifecycle_interface};
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
        if class.r#type != ClassType::ClassDeclaration {
            return;
        }
        for el in &class.body.body {
            let ClassElement::MethodDefinition(m) = el else {
                continue;
            };
            let PropertyKey::StaticIdentifier(id) = &m.key else {
                continue;
            };
            if m.computed || m.kind != MethodDefinitionKind::Method {
                continue;
            }
            let Some(interface) = lifecycle_interface(&id.name) else {
                continue;
            };
            if !implements_name(class, interface) {
                ctx.report(
                    id.span,
                    format!(
                        "Lifecycle interface '{interface}' should be implemented for method '{}'. (https://angular.dev/style-guide#style-09-01)",
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
    fn lifecycle_interface() {
        check(
            "S7655",
            "a.ts",
            r#"
@Component({})
class C implements OnInit {
  ngOnInit() {}
  ngOnDestroy() {} // Noncompliant {{Lifecycle interface 'OnDestroy' should be implemented for method 'ngOnDestroy'. (https://angular.dev/style-guide#style-09-01)}}
  other() {}
}
class D implements ng.OnChanges {
  ngOnChanges() {}
}
"#,
        );
    }
}
