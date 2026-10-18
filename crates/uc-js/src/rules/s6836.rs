//! "case" and "default" clauses should not contain lexical declarations
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchCase]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchCase(case) = node.kind() else {
            return;
        };
        for stmt in &case.consequent {
            let lexical = match stmt {
                Statement::VariableDeclaration(v) => v.kind != VariableDeclarationKind::Var,
                Statement::FunctionDeclaration(_) | Statement::ClassDeclaration(_) => true,
                _ => false,
            };
            if lexical {
                ctx.report(stmt.span(), "Unexpected lexical declaration in case block.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6836",
            "a.js",
            r#"
switch (foo) {
  case 1:
    let x = 1; // Noncompliant {{Unexpected lexical declaration in case block.}}
    break;
  case 2:
    const y = 2; // Noncompliant
    break;
  case 3:
    function f() {} // Noncompliant
    break;
  case 4:
    class C {} // Noncompliant
    break;
  case 5: {
    let z = 1;
    break;
  }
  default:
    var w = 1;
}
"#,
        );
    }
}
