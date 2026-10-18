//! "arguments.caller" and "arguments.callee" should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StaticMemberExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StaticMemberExpression(m) = node.kind() else {
            return;
        };
        let Expression::Identifier(obj) = &m.object else {
            return;
        };
        if obj.name != "arguments" {
            return;
        }
        let prop = m.property.name.as_str();
        if prop == "caller" || prop == "callee" {
            ctx.report(m.span, format!("Avoid arguments.{prop}."));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn caller() {
        check(
            "S2685",
            "a.js",
            r#"
function f() {
  var c = arguments.callee; // Noncompliant {{Avoid arguments.callee.}}
  var d = arguments.caller; // Noncompliant {{Avoid arguments.caller.}}
  var e = arguments.length;
  var g = arguments["callee"];
  var h = foo.callee;
}
"#,
        );
    }
}
