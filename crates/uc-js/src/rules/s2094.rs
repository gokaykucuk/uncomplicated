//! Classes should not be empty
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule {
    allow_constructor_only: bool,
    allow_empty: bool,
    allow_static_only: bool,
    allow_with_decorator: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_constructor_only: p.bool("allowConstructorOnly", false),
            allow_empty: p.bool("allowEmpty", false),
            allow_static_only: p.bool("allowStaticOnly", true),
            allow_with_decorator: p.bool("allowWithDecorator", true),
        }
    }
}

/// Finds `this.x = ...` directly in a constructor body (not inside nested functions / classes).
#[derive(Default)]
struct ThisAssignment {
    found: bool,
}

impl<'a> Visit<'a> for ThisAssignment {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &Class<'a>) {}
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let Some(m) = it.left.as_member_expression()
            && matches!(m.object(), Expression::ThisExpression(_))
        {
            self.found = true;
            return;
        }
        walk::walk_assignment_expression(self, it);
    }
}

fn constructor_assigns_this(class: &Class<'_>) -> bool {
    class.body.body.iter().any(|e| match e {
        ClassElement::MethodDefinition(m) if m.kind == MethodDefinitionKind::Constructor => {
            m.value.body.as_ref().is_some_and(|b| {
                let mut v = ThisAssignment::default();
                v.visit_function_body(b);
                v.found
            })
        }
        _ => false,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        if class.declare
            || class.heritage.is_some()
            || (self.allow_with_decorator && !class.decorators.is_empty())
        {
            return;
        }
        let report_span = class.id.as_ref().map_or(class.span, |id| id.span);
        let members = &class.body.body;
        if members.is_empty() {
            if !self.allow_empty {
                ctx.report(report_span, "Unexpected empty class.");
            }
            return;
        }
        let mut only_static = true;
        let mut only_constructor = true;
        for m in members {
            match m {
                ClassElement::MethodDefinition(md)
                    if md.kind == MethodDefinitionKind::Constructor =>
                {
                    let has_param_property = md
                        .value
                        .params
                        .items
                        .iter()
                        .any(|p| p.accessibility.is_some() || p.readonly || p.r#override);
                    if has_param_property {
                        only_constructor = false;
                        only_static = false;
                    }
                }
                ClassElement::MethodDefinition(md) => {
                    only_constructor = false;
                    if !md.r#static {
                        only_static = false;
                    }
                }
                ClassElement::PropertyDefinition(p) => {
                    only_constructor = false;
                    if !p.r#static {
                        only_static = false;
                    }
                }
                ClassElement::AccessorProperty(p) => {
                    only_constructor = false;
                    if !p.r#static {
                        only_static = false;
                    }
                }
                ClassElement::StaticBlock(_) => only_constructor = false,
                ClassElement::TSIndexSignature(s) => {
                    only_constructor = false;
                    if !s.r#static {
                        only_static = false;
                    }
                }
            }
            if !(only_static || only_constructor) {
                break;
            }
        }
        if only_constructor {
            if !self.allow_constructor_only && !constructor_assigns_this(class) {
                ctx.report(report_span, "Unexpected class with only a constructor.");
            }
        } else if only_static && !self.allow_static_only {
            ctx.report(report_span, "Unexpected class with only static properties.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn extraneous_class() {
        check(
            "S2094",
            "a.ts",
            r#"
class Empty {} // Noncompliant {{Unexpected empty class.}}
class OnlyCtor { // Noncompliant {{Unexpected class with only a constructor.}}
  constructor() { console.log("x"); }
}
class CtorWithState {
  constructor() { this.x = 1; }
}
class CtorWithParamProps {
  constructor(private a: number) {}
}
class OnlyStatic {
  static foo() {}
}
class Derived extends Base {}
@Component({})
class Decorated {}
class Ok { method() {} }
const Anon = class {}; // Noncompliant
"#,
        );
    }

    #[test]
    fn static_only_disallowed() {
        check_with(
            "S2094",
            "a.ts",
            serde_json::json!({"allowStaticOnly": false}),
            r#"
class OnlyStatic { // Noncompliant {{Unexpected class with only static properties.}}
  static foo() {}
}
"#,
        );
    }
}
