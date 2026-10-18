//! Initial values of parameters, caught exceptions, and loop variables should not be ignored
//!
//! For each parameter (in the function body), caught exception (in the catch body) and
//! `for...in`/`for...of` variable (in the loop body), the first reference in source order is
//! inspected: if it overwrites the variable without using its value, the initial value is lost.
use crate::helpers::batch_bug1::contains;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The expression whose value is written by the reference at `id`, as ESLint's `writeExpr`.
fn write_expr<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a Expression<'a>> {
    for n in ctx.ancestors(id) {
        match n.kind() {
            AstKind::ArrayAssignmentTarget(_)
            | AstKind::ObjectAssignmentTarget(_)
            | AstKind::AssignmentTargetRest(_)
            | AstKind::AssignmentTargetWithDefault(_)
            | AstKind::AssignmentTargetPropertyIdentifier(_)
            | AstKind::AssignmentTargetPropertyProperty(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSSatisfiesExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::TSTypeAssertion(_)
            | AstKind::ParenthesizedExpression(_) => {}
            AstKind::AssignmentExpression(a) => return Some(&a.right),
            AstKind::ForInStatement(f) => return Some(&f.right),
            AstKind::ForOfStatement(f) => return Some(&f.right),
            _ => return None,
        }
    }
    None
}

/// Reassignments inside `if` branches or `switch` cases may be shifts or default values.
fn inside_if_or_switch(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut child = ctx.node(id).kind().span();
    for n in ctx.ancestors(id) {
        match n.kind() {
            AstKind::IfStatement(i) => {
                if i.consequent.span() == child
                    || i.alternate.as_ref().is_some_and(|a| a.span() == child)
                {
                    return true;
                }
            }
            AstKind::SwitchCase(_) => return true,
            _ => {}
        }
        child = n.kind().span();
    }
    false
}

fn binding_symbols(p: &BindingPattern<'_>, out: &mut Vec<SymbolId>) {
    for id in p.get_binding_identifiers() {
        out.push(id.symbol_id());
    }
}

fn target_symbols(ctx: &Ctx<'_, '_>, t: &AssignmentTarget<'_>, out: &mut Vec<SymbolId>) {
    match t {
        AssignmentTarget::AssignmentTargetIdentifier(id) => out.extend(ctx.symbol_of(id)),
        AssignmentTarget::ArrayAssignmentTarget(a) => {
            for el in a.elements.iter().flatten() {
                maybe_default_symbols(ctx, el, out);
            }
            if let Some(rest) = &a.rest {
                target_symbols(ctx, &rest.target, out);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(o) => {
            for p in &o.properties {
                match p {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                        out.extend(ctx.symbol_of(&p.binding))
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                        maybe_default_symbols(ctx, &p.binding, out);
                    }
                }
            }
            if let Some(rest) = &o.rest {
                target_symbols(ctx, &rest.target, out);
            }
        }
        _ => {}
    }
}

fn maybe_default_symbols(
    ctx: &Ctx<'_, '_>,
    t: &AssignmentTargetMaybeDefault<'_>,
    out: &mut Vec<SymbolId>,
) {
    match t {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            target_symbols(ctx, &d.binding, out)
        }
        _ => {
            if let Some(t) = t.as_assignment_target() {
                target_symbols(ctx, t, out);
            }
        }
    }
}

fn left_symbols(ctx: &Ctx<'_, '_>, left: &ForStatementLeft<'_>, out: &mut Vec<SymbolId>) {
    match left {
        ForStatementLeft::VariableDeclaration(d) => {
            for decl in &d.declarations {
                binding_symbols(&decl.id, out);
            }
        }
        _ => {
            if let Some(t) = left.as_assignment_target() {
                target_symbols(ctx, t, out);
            }
        }
    }
}

/// Offset of the first `arguments` use whose nearest enclosing function is `func`.
fn first_arguments_use(ctx: &Ctx<'_, '_>, func: NodeId, region: Span) -> u32 {
    let Some(refs) = ctx.scoping().root_unresolved_references().get("arguments") else {
        return u32::MAX;
    };
    let mut first = u32::MAX;
    for rid in refs.iter() {
        let node_id = ctx.scoping().get_reference(*rid).node_id();
        let span = ctx.node(node_id).kind().span();
        if span.start < first
            && contains(region, span)
            && ctx
                .enclosing_function(node_id)
                .is_some_and(|f| f.id() == func)
        {
            first = span.start;
        }
    }
    first
}

fn check_symbol(ctx: &Ctx<'_, '_>, sym: SymbolId, region: Span, limit: &mut dyn FnMut() -> u32) {
    let scoping = ctx.scoping();
    // Fast path: most variables are read first, which needs no further work.
    let mut first: Option<(u32, bool)> = None;
    for r in scoping.get_resolved_references(sym) {
        let span = ctx.node(r.node_id()).kind().span();
        if contains(region, span) && first.is_none_or(|(s, _)| span.start < s) {
            first = Some((span.start, r.is_write() && !r.is_read()));
        }
    }
    if !first.is_some_and(|(_, write_only)| write_only) {
        return;
    }
    let limit = limit();
    let mut refs: Vec<(Span, NodeId, bool)> = scoping
        .get_resolved_references(sym)
        .filter_map(|r| {
            let span = ctx.node(r.node_id()).kind().span();
            contains(region, span).then(|| (span, r.node_id(), r.is_write() && !r.is_read()))
        })
        .collect();
    refs.sort_unstable_by_key(|(s, _, _)| s.start);
    let name = scoping.symbol_name(sym);
    for (span, node_id, write_only) in refs {
        if span.start >= limit || !write_only {
            return;
        }
        if inside_if_or_switch(ctx, node_id) {
            continue;
        }
        let expr = write_expr(ctx, node_id);
        if let Some(e) = expr
            && ctx.tokens_in(e.span()).iter().any(|t| {
                let text = ctx.text(t.span());
                text == name || text == "arguments"
            })
        {
            return;
        }
        let loc = expr.map_or(span, |e| Span::new(span.start, e.span().end));
        ctx.report(
            loc,
            format!(
                "Introduce a new variable or use its initial value before reassigning \"{name}\"."
            ),
        );
        return;
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::CatchClause,
            AstType::ForInStatement,
            AstType::ForOfStatement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let mut symbols: Vec<SymbolId> = Vec::new();
        let (region, is_function) = match node.kind() {
            AstKind::Function(f) => {
                let Some(body) = &f.body else { return };
                for p in f.params.iter_bindings() {
                    binding_symbols(p, &mut symbols);
                }
                if let Some(id) = &f.id {
                    symbols.retain(|s| ctx.scoping().symbol_name(*s) != id.name.as_str());
                }
                (body.span, true)
            }
            AstKind::ArrowFunctionExpression(a) => {
                let Some(body) = a.get_function_body() else {
                    return;
                };
                if a.get_expression().is_some() {
                    return;
                }
                for p in a.params.iter_bindings() {
                    binding_symbols(p, &mut symbols);
                }
                (body.span, true)
            }
            AstKind::CatchClause(c) => {
                let Some(p) = &c.param else { return };
                binding_symbols(&p.pattern, &mut symbols);
                (c.body.span, false)
            }
            AstKind::ForInStatement(f) => {
                left_symbols(ctx, &f.left, &mut symbols);
                (f.body.span(), false)
            }
            AstKind::ForOfStatement(f) => {
                left_symbols(ctx, &f.left, &mut symbols);
                (f.body.span(), false)
            }
            _ => return,
        };
        if symbols.is_empty() {
            return;
        }
        let mut limit_cache: Option<u32> = None;
        let mut limit = || {
            *limit_cache.get_or_insert_with(|| {
                if is_function { first_arguments_use(ctx, node.id(), region) } else { u32::MAX }
            })
        };
        for sym in symbols {
            check_symbol(ctx, sym, region, &mut limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn compliant() {
        check(
            "S1226",
            "a.ts",
            r#"
function foo(p1, p2, p3, ...p4) {
  p1.prop1 = 42;
  foo(p2, p3);
  var x = p4 + 1;
  p1 = 3;
  p2 = 3;
}
function foobar(opts: any, p1: number, p2: number) {
  opts = opts || {};
  p1 += p1;
  p2 = doSomething(p2);
}
function myFunc2(p1) {
  if (someBoolean) {
    p1 = "defaultValue";
  } else {
    p1 = "otherValue";
  }
}
function myFunc3(p1) {
  p1 = arguments[1] || arguments[0];
}
try {
  foo();
} catch (e) {
  if (e === e1) { e = 0; }
  e = 3;
}
for (var x in obj) {
  if (abc) { let a = x; }
  x = 3;
}
function mixedParams(param1) {
  var val = param1;
  for (var attr in attributes) {
    param1 = t.names[attr] || attr;
  }
}
for (let c = 0, d = 1; c < 3; c++) {
  c = foo();
}
function withArgs(p1) {
  MyClass.prototype.functionToCall.apply(this, arguments);
  p1 = this.position;
}
function someFunction(node, param = false) {
  switch (node.type) {
    case 'ForStatement':
      param = true;
      break;
  }
}
const exprArrow = (p) => p = 1;
"#,
        );
    }

    #[test]
    fn noncompliant() {
        check(
            "S1226",
            "a.js",
            r#"
function foo(p1) {
  p1 = 42; // Noncompliant {{Introduce a new variable or use its initial value before reassigning "p1".}}
}
function foo2(p1) {
  while (someBoolean) {
    if (p1 = doSomething()) return p1; // Noncompliant
  }
}
function foo3(p1) {
  if (someBoolean) {
    p1 = "defaultValue";
  }
  p1 = "newValue"; // Noncompliant
}
function bindingElements({ a: p1 = 1, p2 = 2 }, [p3 = 3, p4 = 4], p5 = 5) {
  p1 = 42; // Noncompliant
  p2 = 42; // Noncompliant
  p3 = 42; // Noncompliant
  p4 = 42; // Noncompliant
  p5 = 42; // Noncompliant
  p5 = 42;
}
var arrow1 = (p1, p2) => {
  p2 = 42; // Noncompliant
  foo(p1, p2);
};
try {
  foo();
} catch ([e, e1, e2]) {
  e = foo(); // Noncompliant
  e1 = foo(); // Noncompliant
  foo(e2);
}
for (var x in obj) {
  for (let x in obj) {
    x = foo(); // Noncompliant
  }
  x = foo(); // Noncompliant
}
var y = 1;
y = 42;
for (y of obj) {
  y = foo(); // Noncompliant
}
var a, b;
for ({ a, b } in obj) {
  a = foo(); // Noncompliant
  b = foo(); // Noncompliant
}
function f1(p1) {
  function f2(p2) {
    var args = arguments[0];
    function f3(p3) {
      p1 = 1; // Noncompliant
      p2 = 1;
      p3 = 1; // Noncompliant
    }
  }
}
"#,
        );
    }
}
