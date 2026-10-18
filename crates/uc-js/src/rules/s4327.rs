//! "this" should not be assigned to variables
use crate::prelude::*;

pub struct Rule {
    allow_destructuring: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_destructuring: p.bool("allowDestructuring", true),
        }
    }
}

/// True if some reference to `sym` is made from inside a generator nested in the variable's scope.
fn referenced_inside_generator(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    let scope_node = ctx
        .scoping()
        .get_node_id(ctx.scoping().symbol_scope_id(sym));
    ctx.scoping().get_resolved_references(sym).any(|r| {
        for a in std::iter::once(ctx.node(r.node_id())).chain(ctx.ancestors(r.node_id())) {
            if a.id() == scope_node {
                return false;
            }
            if let AstKind::Function(f) = a.kind()
                && f.generator
            {
                return true;
            }
        }
        false
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator, AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (span, sym, is_ident) = match node.kind() {
            AstKind::VariableDeclarator(d) => {
                if !matches!(
                    d.init.as_ref().map(strip_parens),
                    Some(Expression::ThisExpression(_))
                ) {
                    return;
                }
                match &d.id {
                    BindingPattern::BindingIdentifier(id) => (id.span, id.symbol_id.get(), true),
                    p => (p.span(), None, false),
                }
            }
            AstKind::AssignmentExpression(a) => {
                if !matches!(strip_parens(&a.right), Expression::ThisExpression(_)) {
                    return;
                }
                match &a.left {
                    AssignmentTarget::AssignmentTargetIdentifier(id) => {
                        (id.span, ctx.symbol_of(id), true)
                    }
                    t => (t.span(), None, false),
                }
            }
            _ => return,
        };
        if !is_ident && self.allow_destructuring {
            return;
        }
        if let Some(sym) = sym
            && referenced_inside_generator(ctx, sym)
        {
            return;
        }
        let msg = if is_ident {
            "Unexpected aliasing of 'this' to local variable."
        } else {
            "Unexpected aliasing of members of 'this' to local variables."
        };
        ctx.report(span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn this_alias() {
        check(
            "S4327",
            "a.ts",
            r#"
function f() {
  const self = this; // Noncompliant {{Unexpected aliasing of 'this' to local variable.}}
  let that;
  that = this; // Noncompliant
  const { props } = this;
  setTimeout(() => self.x + that.y);
}
class C {
  *gen() {}
  m() {
    const self = this;
    return function* () {
      yield self.x;
    };
  }
}
"#,
        );
        check_with(
            "S4327",
            "a.ts",
            serde_json::json!({"allowDestructuring": false}),
            r#"
const { props } = this; // Noncompliant {{Unexpected aliasing of members of 'this' to local variables.}}
"#,
        );
    }
}
