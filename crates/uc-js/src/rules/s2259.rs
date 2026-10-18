//! Properties of variables with "null" or "undefined" values should not be accessed
//!
//! Sonar relies on the TypeScript checker's flow-sensitive type of the dereferenced identifier.
//! Without it, we report the cases where that type is certainly `null`/`undefined`: local
//! variables declared without initializer (or initialized to `null`/`undefined`) that are never
//! written afterwards, dereferenced in the function that declares them. The short-circuit check
//! (`x == null && x.foo`) is purely syntactic.
use oxc_ast_visit::{Visit, walk};

use crate::helpers::batch_security1::{enclosing_fn, parents};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_nullish_literal(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    is_null(e)
        || matches!(strip_parens(e), Expression::Identifier(id) if ctx.is_global(id, "undefined"))
}

/// The symbol certainly holds `null` / `undefined` at every read.
fn is_always_nullish(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return false;
    };
    if !matches!(&v.id, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym)) {
        return false;
    }
    if !v.init.as_ref().is_none_or(|i| is_nullish_literal(ctx, i)) {
        return false;
    }
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() || ctx.scoping().symbol_is_mutated(sym)
    {
        return false;
    }
    // `for (x of ...)`-style declarations are written by the loop.
    if matches!(
        ctx.parent_kind(ctx.parent(decl.id()).id()),
        AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
    ) {
        return false;
    }
    if matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.declare) {
        return false;
    }
    true
}

struct MemberFinder<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    target: Span,
    found: Vec<Span>,
}

impl<'a> MemberFinder<'_, 'a, '_> {
    fn check(&mut self, object: &Expression<'a>, optional: bool) {
        if optional {
            return;
        }
        let o = strip_parens(object).span();
        if self.ctx.same_tokens(o, self.target) {
            self.found.push(o);
        }
    }
}

impl<'a> Visit<'a> for MemberFinder<'_, 'a, '_> {
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        self.check(&it.object, it.optional);
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        self.check(&it.object, it.optional);
        walk::walk_computed_member_expression(self, it);
    }
}

impl Rule {
    fn check_dereference(
        &self,
        ctx: &Ctx<'_, '_>,
        node_id: NodeId,
        object: &Expression<'_>,
        state: &mut FxHashMap<SymbolId, bool>,
    ) {
        let Expression::Identifier(id) = object else {
            return;
        };
        let Some(sym) = ctx.symbol_of(id) else { return };
        // `true` once checked: either reported already or known not to be nullish.
        if *state
            .entry(sym)
            .or_insert_with(|| !is_always_nullish(ctx, sym))
        {
            return;
        }
        let decl_fn = enclosing_fn(ctx, ctx.symbol_decl(sym).id());
        if enclosing_fn(ctx, node_id) != decl_fn {
            return;
        }
        state.insert(sym, true);
        ctx.report(
            id.span,
            format!(
                "TypeError can be thrown as \"{}\" might be null or undefined here.",
                id.name
            ),
        );
    }

    fn check_short_circuit<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        node: &AstNode<'a>,
        l: &LogicalExpression<'a>,
    ) {
        let Expression::BinaryExpression(b) = strip_parens(&l.left) else {
            return;
        };
        let (target, confirmed) = match b.operator {
            BinaryOperator::Equality | BinaryOperator::StrictEquality => (true, true),
            BinaryOperator::Inequality | BinaryOperator::StrictInequality => (true, false),
            _ => (false, false),
        };
        if !target {
            return;
        }
        let compared = if is_nullish_literal(ctx, &b.right) {
            &b.left
        } else if is_nullish_literal(ctx, &b.left) {
            &b.right
        } else {
            return;
        };
        let triggers = (confirmed && l.operator == LogicalOperator::And)
            || (!confirmed && l.operator == LogicalOperator::Or);
        if !triggers {
            return;
        }
        // Sonar considers the outermost logical expression only.
        if parents(ctx, node.id()).any(|a| matches!(a.kind(), AstKind::LogicalExpression(_))) {
            return;
        }
        let mut finder = MemberFinder {
            ctx,
            target: strip_parens(compared).span(),
            found: Vec::new(),
        };
        finder.visit_logical_expression(l);
        for s in finder.found {
            ctx.report(
                s,
                "TypeError can be thrown as expression might be null or undefined here.",
            );
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut state: FxHashMap<SymbolId, bool> = FxHashMap::default();
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::StaticMemberExpression(m) if !m.optional => {
                    self.check_dereference(ctx, node.id(), &m.object, &mut state)
                }
                AstKind::ComputedMemberExpression(m) if !m.optional => {
                    self.check_dereference(ctx, node.id(), &m.object, &mut state)
                }
                AstKind::ForOfStatement(f) => {
                    self.check_dereference(ctx, node.id(), &f.right, &mut state)
                }
                AstKind::LogicalExpression(l) => self.check_short_circuit(ctx, node, l),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn null_dereference() {
        check(
            "S2259",
            "a.js",
            r#"
function property() {
  var x;
  x.foo; // Noncompliant {{TypeError can be thrown as "x" might be null or undefined here.}}
  x.bar;
}
function element() {
  var y = null;
  y[1]; // Noncompliant
}
function assigned() {
  var z;
  z = foo();
  z.foo;
}
function forOf() {
  var arr;
  for (const i of arr) {} // Noncompliant
  var x;
  for (x of obj) {}
}
function inner() {
  let w;
  return () => w.foo;
}
function ok() {
  const s = "str";
  s.trim();
}
if (x == null && x.prop == 0) {} // Noncompliant {{TypeError can be thrown as expression might be null or undefined here.}}
if (null != x || x.prop == 0) {} // Noncompliant
if (x.prop != undefined || x.prop.prop2 == 0) {} // Noncompliant
if (x != null && x.prop) {}
if (a && (x == null && x.foo)) {}
"#,
        );
    }
}
