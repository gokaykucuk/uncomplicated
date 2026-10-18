//! Values should not be uselessly incremented
use crate::helpers::batch_bug1::parent_skip_parens_only;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report(ctx: &Ctx<'_, '_>, u: &UpdateExpression<'_>) {
    let op = if u.operator == UpdateOperator::Increment {
        "inc"
    } else {
        "dec"
    };
    ctx.report(
        u.span,
        format!("Remove this {op}rement or correct the code not to waste it."),
    );
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UpdateExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::UpdateExpression(u) = node.kind() else {
            return;
        };
        if u.prefix {
            return;
        }
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(arg) = &u.argument else {
            return;
        };
        match parent_skip_parens_only(ctx, node.id()).kind() {
            // `return i++;` where `i` is declared in the innermost scope.
            AstKind::ReturnStatement(_) => {
                if let Some(sym) = ctx.symbol_of(arg)
                    && ctx.scoping().symbol_scope_id(sym) == node.scope_id()
                {
                    report(ctx, u);
                }
            }
            // `i = i++;`
            AstKind::AssignmentExpression(a) => {
                if let AssignmentTarget::AssignmentTargetIdentifier(lhs) = &a.left
                    && lhs.name == arg.name
                {
                    report(ctx, u);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_increment() {
        check(
            "S2123",
            "a.js",
            r#"
function pickNumber() {
  let i = 0;
  i = i++; // Noncompliant {{Remove this increment or correct the code not to waste it.}}
  i = i--; // Noncompliant {{Remove this decrement or correct the code not to waste it.}}
  return i++; // Noncompliant
}
function f(i) { return i++; } // Noncompliant
let j = 0;
function g() { return j++; }
function h() { let k = 0; if (x) { return k++; } }
function ok() { let i = 0; i = ++i; i = j++; return ++i; }
"#,
        );
    }
}
