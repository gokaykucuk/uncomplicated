//! Parameters should be passed in the correct order
//!
//! Callees are resolved locally (function declarations, functions assigned to never-reassigned
//! variables, inline functions). Swapped arguments must have the same type: either the same
//! known type, or both untyped parameters (implicitly `any`).
use crate::helpers::batch_smell2::{parent_skip_parens_only, wordish_contains};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DIRECTIONAL: &[&str] = &[
    "rtl", "ltr", "reverse", "flip", "swap", "forward", "backward",
];

/// What an operand stands for: a resolved symbol or a global name.
#[derive(PartialEq, Clone, Copy)]
enum Op<'s> {
    Sym(SymbolId),
    Name(&'s str),
}

fn ident_op<'s>(ctx: &Ctx<'_, '_>, id: &'s IdentifierReference<'_>) -> Op<'s> {
    ctx.symbol_of(id)
        .map_or(Op::Name(id.name.as_str()), Op::Sym)
}

fn param_name<'b>(p: &'b BindingPattern<'_>) -> Option<&'b str> {
    fn collect<'b>(p: &'b BindingPattern<'_>, out: &mut Vec<&'b str>) {
        match p {
            BindingPattern::BindingIdentifier(id) => out.push(id.name.as_str()),
            BindingPattern::AssignmentPattern(a) => collect(&a.left, out),
            BindingPattern::ArrayPattern(a) => {
                for e in a.elements.iter().flatten() {
                    collect(e, out);
                }
                if let Some(r) = &a.rest {
                    collect(&r.argument, out);
                }
            }
            BindingPattern::ObjectPattern(o) => {
                for prop in &o.properties {
                    if !prop.shorthand {
                        collect(&prop.value, out);
                    }
                }
                if let Some(r) = &o.rest {
                    collect(&r.argument, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    collect(p, &mut out);
    if out.len() == 1 { Some(out[0]) } else { None }
}

fn param_names<'b>(params: &'b FormalParameters<'_>) -> Vec<Option<&'b str>> {
    let mut v: Vec<Option<&str>> = params
        .items
        .iter()
        .map(|p| param_name(&p.pattern))
        .collect();
    if let Some(r) = &params.rest {
        v.push(param_name(&r.rest.argument));
    }
    v
}

/// Formal parameters of the called function, when it can be resolved locally.
fn resolve_params<'b, 'a>(
    ctx: &'b Ctx<'a, '_>,
    callee: &'b Expression<'a>,
) -> Option<&'b FormalParameters<'a>> {
    match strip_parens(callee) {
        Expression::FunctionExpression(f) => Some(&f.params),
        Expression::ArrowFunctionExpression(a) => Some(&a.params),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) if f.is_declaration() && f.body.is_some() => Some(&f.params),
                AstKind::VariableDeclarator(d) if !ctx.scoping().symbol_is_mutated(sym) => {
                    match d.init.as_ref().map(strip_parens) {
                        Some(Expression::FunctionExpression(f)) => Some(&f.params),
                        Some(Expression::ArrowFunctionExpression(a)) => Some(&a.params),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// An untyped, default-less function parameter (implicitly `any` for TypeScript).
fn is_untyped_param(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    matches!(ctx.symbol_decl(sym).kind(), AstKind::FormalParameter(p)
        if p.type_annotation.is_none() && p.initializer.is_none() && matches!(p.pattern, BindingPattern::BindingIdentifier(_)))
}

fn is_const_binding(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    ctx.scoping()
        .symbol_flags(sym)
        .contains(SymbolFlags::ConstVariable)
}

fn compatible_types(
    ctx: &Ctx<'_, '_>,
    a: &IdentifierReference<'_>,
    b: &IdentifierReference<'_>,
) -> bool {
    // `const` bindings get literal types from their initializers: we can't compare those.
    if is_const_binding(ctx, a) || is_const_binding(ctx, b) {
        return false;
    }
    let ty = |id: &IdentifierReference<'_>| {
        ctx.symbol_of(id)
            .map_or(Ty::Unknown, |s| helpers::types::symbol_type(ctx, s, 0))
    };
    let (ta, tb) = (ty(a), ty(b));
    if !ta.is_unknown() && !tb.is_unknown() {
        return ta == tb;
    }
    is_untyped_param(ctx, a) && is_untyped_param(ctx, b)
}

fn is_comparison(op: BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        op,
        Equality
            | Inequality
            | StrictEquality
            | StrictInequality
            | LessThan
            | LessEqualThan
            | GreaterThan
            | GreaterEqualThan
    )
}

/// What a compared side reads: `a`, `a.length`, `a.foo()` all read `a`.
fn operand<'s>(ctx: &Ctx<'_, '_>, e: &'s Expression<'_>) -> Option<Op<'s>> {
    match strip_parens(e) {
        Expression::Identifier(id) => Some(ident_op(ctx, id)),
        Expression::CallExpression(c) => operand(ctx, &c.callee),
        Expression::StaticMemberExpression(m) => operand(ctx, &m.object),
        Expression::ComputedMemberExpression(m) => operand(ctx, &m.object),
        _ => None,
    }
}

/// `const m = a.length` makes `m` stand for `a`.
fn member_snapshot(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<SymbolId> {
    let Expression::Identifier(id) = strip_parens(e) else {
        return None;
    };
    let sym = ctx.symbol_of(id)?;
    if !ctx
        .scoping()
        .symbol_flags(sym)
        .contains(SymbolFlags::ConstVariable)
    {
        return None;
    }
    let AstKind::VariableDeclarator(d) = ctx.symbol_decl(sym).kind() else {
        return None;
    };
    if !matches!(d.id, BindingPattern::BindingIdentifier(_)) {
        return None;
    }
    let obj = match d.init.as_ref().map(strip_parens)? {
        Expression::StaticMemberExpression(m) => &m.object,
        Expression::ComputedMemberExpression(m) => &m.object,
        _ => return None,
    };
    let Expression::Identifier(o) = obj else {
        return None;
    };
    ctx.symbol_of(o)
}

fn compared_sides(ctx: &Ctx<'_, '_>, call_id: NodeId, first: Op<'_>, second: Op<'_>) -> bool {
    let Some(if_stmt) = ctx.nodes().ancestors(call_id).find_map(|a| match a.kind() {
        AstKind::IfStatement(i) => Some(i),
        _ => None,
    }) else {
        return false;
    };
    let mut test = strip_parens(&if_stmt.test);
    if let Expression::UnaryExpression(u) = test
        && u.operator == UnaryOperator::LogicalNot
    {
        test = strip_parens(&u.argument);
    }
    let (lhs, rhs) = match test {
        Expression::BinaryExpression(b) if is_comparison(b.operator) => (&b.left, &b.right),
        Expression::CallExpression(c) if c.arguments.len() == 1 => {
            let Some(m) = c.callee.as_member_expression() else {
                return false;
            };
            let Some(arg) = c.arguments[0].as_expression() else {
                return false;
            };
            (m.object(), arg)
        }
        _ => return false,
    };
    let l: Vec<Op<'_>> = [operand(ctx, lhs), member_snapshot(ctx, lhs).map(Op::Sym)]
        .into_iter()
        .flatten()
        .collect();
    let r: Vec<Op<'_>> = [operand(ctx, rhs), member_snapshot(ctx, rhs).map(Op::Sym)]
        .into_iter()
        .flatten()
        .collect();
    (l.contains(&first) && r.contains(&second)) || (l.contains(&second) && r.contains(&first))
}

fn call_arguments<'b, 'a>(
    kind: &'b AstKind<'a>,
) -> Option<(&'b Expression<'a>, &'b [Argument<'a>])> {
    match kind {
        AstKind::CallExpression(c) => Some((&c.callee, c.arguments.as_slice())),
        AstKind::NewExpression(n) => Some((&n.callee, n.arguments.as_slice())),
        _ => None,
    }
}

fn arg_ident<'b, 'a>(a: &'b Argument<'a>) -> Option<&'b IdentifierReference<'a>> {
    match a {
        Argument::Identifier(id) => Some(id),
        _ => None,
    }
}

/// `(a, b) => compare(b, a)`: an intentional comparator reversal.
fn is_comparator_reversal(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, a1: &str, a2: &str) -> bool {
    let call_span = node.kind().span();
    for anc in ctx.nodes().ancestors(node.id()) {
        let (params, body_is_call) = match anc.kind() {
            AstKind::ArrowFunctionExpression(f) => {
                let is_call = match &f.body {
                    ArrowFunctionBody::FunctionBody(b) => match b.statements.as_slice() {
                        [Statement::ReturnStatement(r)] => {
                            r.argument.as_ref().is_some_and(|a| a.span() == call_span)
                        }
                        _ => false,
                    },
                    body => body
                        .as_expression()
                        .is_some_and(|e| strip_parens(e).span() == call_span),
                };
                (&f.params, is_call)
            }
            AstKind::Function(f) if !f.is_declaration() => {
                let is_call = f
                    .body
                    .as_ref()
                    .is_some_and(|b| match b.statements.as_slice() {
                        [Statement::ReturnStatement(r)] => {
                            r.argument.as_ref().is_some_and(|a| a.span() == call_span)
                        }
                        _ => false,
                    });
                (&f.params, is_call)
            }
            _ => continue,
        };
        if params.items.len() != 2 || params.rest.is_some() {
            return false;
        }
        let names: Vec<&str> = params
            .items
            .iter()
            .filter_map(|p| match &p.pattern {
                BindingPattern::BindingIdentifier(id) if p.initializer.is_none() => {
                    Some(id.name.as_str())
                }
                _ => None,
            })
            .collect();
        if names.len() != 2 || names.iter().any(|n| n.len() > 1) {
            return false;
        }
        return names.contains(&a1) && names.contains(&a2) && body_is_call;
    }
    false
}

fn in_directional_context(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    ctx.nodes().ancestors(node.id()).any(|a| match a.kind() {
        AstKind::ObjectProperty(p) => match &p.key {
            PropertyKey::StaticIdentifier(id) => wordish_contains(&id.name, DIRECTIONAL),
            PropertyKey::StringLiteral(s) => wordish_contains(&s.value, DIRECTIONAL),
            _ => false,
        },
        _ => false,
    })
}

/// `a < b ? fn(a, b) : fn(b, a)`.
fn is_ternary_swap(
    ctx: &Ctx<'_, '_>,
    node: &AstNode<'_>,
    args: &[Argument<'_>],
    callee: &Expression<'_>,
    a1: &str,
    a2: &str,
) -> bool {
    let AstKind::ConditionalExpression(c) = parent_skip_parens_only(ctx, node.id()).kind() else {
        return false;
    };
    let span = node.kind().span();
    let other = if strip_parens(&c.consequent).span() == span {
        &c.alternate
    } else if strip_parens(&c.alternate).span() == span {
        &c.consequent
    } else {
        return false;
    };
    let Expression::CallExpression(other) = strip_parens(other) else {
        return false;
    };
    if ctx.text_of(&other.callee) != ctx.text_of(callee) || other.arguments.len() != args.len() {
        return false;
    }
    let pos = |name: &str| {
        args.iter()
            .position(|a| arg_ident(a).is_some_and(|i| i.name == name))
    };
    let (Some(i1), Some(i2)) = (pos(a1), pos(a2)) else {
        return false;
    };
    let named = |i: usize, name: &str| {
        other
            .arguments
            .get(i)
            .and_then(arg_ident)
            .is_some_and(|id| id.name == name)
    };
    if !(named(i1, a2) && named(i2, a1)) {
        return false;
    }
    let Expression::BinaryExpression(b) = strip_parens(&c.test) else {
        return false;
    };
    if !is_comparison(b.operator) {
        return false;
    }
    let (Expression::Identifier(l), Expression::Identifier(r)) = (&b.left, &b.right) else {
        return false;
    };
    let names = [l.name.as_str(), r.name.as_str()];
    names.contains(&a1) && names.contains(&a2)
}

/// MD5-style round functions `ff(a, b, c, d, ...)` rotate their state arguments on purpose.
fn is_crypto_rotation(
    callee: &Expression<'_>,
    params: &[Option<&str>],
    args: &[Option<&str>],
) -> bool {
    let name = match strip_parens(callee) {
        Expression::Identifier(id) => id.name.as_str(),
        Expression::StaticMemberExpression(m) => m.property.name.as_str(),
        _ => return false,
    };
    let lower = name.to_ascii_lowercase();
    let core = lower
        .strip_prefix("md5_")
        .or_else(|| lower.strip_prefix("md4_"))
        .or_else(|| lower.strip_prefix("md5"))
        .or_else(|| lower.strip_prefix("md4"))
        .unwrap_or(&lower);
    if !matches!(core, "ff" | "gg" | "hh" | "ii") || params.len() < 4 || args.len() < 4 {
        return false;
    }
    let Some(args4) = args[..4].iter().copied().collect::<Option<Vec<&str>>>() else {
        return false;
    };
    let Some(params4) = params[..4].iter().copied().collect::<Option<Vec<&str>>>() else {
        return false;
    };
    (1..4).any(|k| (0..4).all(|i| args4[i] == params4[(i + k) % 4]))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        let Some((callee, args)) = call_arguments(&kind) else {
            return;
        };
        let arg_names: Vec<Option<&str>> = args
            .iter()
            .map(|a| arg_ident(a).map(|i| i.name.as_str()))
            .collect();
        if arg_names.iter().flatten().count() < 2 {
            return;
        }
        let Some(params) = resolve_params(ctx, callee) else {
            return;
        };
        let param_names = param_names(params);
        if is_crypto_rotation(callee, &param_names, &arg_names) {
            return;
        }
        for (i, name) in arg_names.iter().enumerate() {
            let Some(name) = *name else { continue };
            let Some(idx) = param_names.iter().position(|p| *p == Some(name)) else {
                continue;
            };
            if idx == i {
                continue;
            }
            let Some(Some(swapped)) = arg_names.get(idx).copied() else {
                continue;
            };
            if param_names.get(i).copied().flatten() != Some(swapped) {
                continue;
            }
            let (Some(id1), Some(id2)) = (arg_ident(&args[i]), arg_ident(&args[idx])) else {
                continue;
            };
            if !compatible_types(ctx, id1, id2) {
                continue;
            }
            if compared_sides(ctx, node.id(), ident_op(ctx, id1), ident_op(ctx, id2))
                || is_comparator_reversal(ctx, node, name, swapped)
                || in_directional_context(ctx, node)
                || is_ternary_swap(ctx, node, args, callee, name, swapped)
            {
                continue;
            }
            let loc = Span::new(args[0].span().start, args[args.len() - 1].span().end);
            let mut secondary = Vec::new();
            let all: Vec<Span> = params
                .items
                .iter()
                .map(|p| p.span)
                .chain(params.rest.iter().map(|r| r.span))
                .collect();
            if let (Some(f), Some(l)) = (all.first(), all.last()) {
                secondary.push((
                    Span::new(f.start, l.end),
                    Some("Formal parameters".to_string()),
                ));
            }
            ctx.report_with(
                loc,
                format!(
                    "Arguments '{name}' and '{swapped}' have the same names but not the same order as the function parameters."
                ),
                secondary,
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn arguments_order() {
        check(
            "S2234",
            "a.ts",
            r#"
function divide(divisor: number, dividend: number) { return divisor / dividend; }
function doTheThing(divisor: number, dividend: number) {
  divide(dividend, divisor); // Noncompliant {{Arguments 'dividend' and 'divisor' have the same names but not the same order as the function parameters.}}
  divide(divisor, dividend);
}
function mixed(divisor: string, dividend: number) {
  divide(dividend, divisor);
}
function untyped(a, b) {}
function callUntyped(a, b) {
  untyped(b, a); // Noncompliant
  if (a < b) { untyped(b, a); }
  if (a.length > b.length) { untyped(b, a); }
}
function viaComparator(a, b) {
  return [1].sort((a, b) => untyped(b, a));
}
function viaTernary(a, b) {
  return a < b ? untyped(a, b) : untyped(b, a);
}
function viaFlag(a, b, flag) {
  return flag ? untyped(b, a) : untyped(a, b); // Noncompliant
}
function directional(a, b) {
  return { rtl: () => untyped(b, a) };
}
function md5(a, b, c, d) {
  ff(b, c, d, a);
}
function ff(a, b, c, d) {}
function unresolved(a, b) { obj.method(b, a); }
"#,
        );
    }
}
