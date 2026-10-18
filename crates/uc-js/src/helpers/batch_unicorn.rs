//! Helpers owned by the `unicorn` rule batch.
//!
//! Most rules of this batch wrap eslint-plugin-unicorn rules, which work on ESTree. These helpers
//! mirror the small predicates unicorn uses (`isMethodCall`, `isMemberExpression`, ...) on the oxc
//! AST. ESTree has no parenthesized-expression nodes, so parentheses are skipped where unicorn
//! would see through them.

use oxc_ast::AstKind;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::strip_parens;

/// `obj.name(...)` with a non-computed member callee: returns the member and the method name.
pub fn method_call<'b, 'a>(
    call: &'b CallExpression<'a>,
) -> Option<(&'b StaticMemberExpression<'a>, &'a str)> {
    match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m) => Some((m, m.property.name.as_str())),
        _ => None,
    }
}

/// True if no argument is a spread element.
pub fn no_spread(args: &[Argument<'_>]) -> bool {
    !args.iter().any(|a| matches!(a, Argument::SpreadElement(_)))
}

/// Unicorn's `isMethodCall(node, {methods, argumentsLength, optionalCall: false, optionalMember: false})`.
/// `args`: required argument count (spread elements are refused).
pub fn is_method_call<'b, 'a>(
    call: &'b CallExpression<'a>,
    methods: &[&str],
    args: Option<usize>,
    allow_optional: bool,
) -> Option<&'b StaticMemberExpression<'a>> {
    let (m, name) = method_call(call)?;
    if !methods.contains(&name) {
        return None;
    }
    if !allow_optional && (call.optional || m.optional) {
        return None;
    }
    if let Some(n) = args
        && (call.arguments.len() != n || !no_spread(&call.arguments))
    {
        return None;
    }
    Some(m)
}

/// Argument as an expression (parens stripped), `None` for spread elements.
pub fn arg<'b, 'a>(args: &'b [Argument<'a>], i: usize) -> Option<&'b Expression<'a>> {
    args.get(i)
        .and_then(|a| a.as_expression())
        .map(strip_parens)
}

/// True if `e` (parens stripped) is an identifier named `name` that resolves to no local binding.
pub fn is_global_ref(ctx: &Ctx<'_, '_>, e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if ctx.is_global(id, name))
}

/// True if `e` (parens stripped) is an identifier named `name` (any binding).
pub fn is_ident(e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == name)
}

/// Static member `object.property` where `object` is a global identifier.
pub fn is_global_member(
    ctx: &Ctx<'_, '_>,
    e: &Expression<'_>,
    object: &str,
    property: &str,
) -> bool {
    matches!(strip_parens(e), Expression::StaticMemberExpression(m)
        if m.property.name == property && is_global_ref(ctx, &m.object, object))
}

/// Numeric literal value.
pub fn number_value(e: &Expression<'_>) -> Option<f64> {
    match strip_parens(e) {
        Expression::NumericLiteral(n) => Some(n.value),
        _ => None,
    }
}

/// True for `x.length` (non-computed, non-optional); returns `x`.
pub fn length_of<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) if m.property.name == "length" && !m.optional => {
            Some(&m.object)
        }
        _ => None,
    }
}

/// Span from `start` of `a` to end of `b`.
pub fn join(a: Span, b: Span) -> Span {
    Span::new(a.start, b.end)
}

/// Checks two expressions are the same reference chain (`a`, `a.b`, `this.x[0]`) syntactically.
pub fn same_reference(ctx: &Ctx<'_, '_>, a: &Expression<'_>, b: &Expression<'_>) -> bool {
    let (a, b) = (strip_parens(a), strip_parens(b));
    match (a, b) {
        (Expression::Identifier(x), Expression::Identifier(y)) => x.name == y.name,
        (Expression::ThisExpression(_), Expression::ThisExpression(_)) => true,
        (Expression::Super(_), Expression::Super(_)) => true,
        (Expression::StaticMemberExpression(x), Expression::StaticMemberExpression(y)) => {
            x.property.name == y.property.name
                && x.optional == y.optional
                && same_reference(ctx, &x.object, &y.object)
        }
        (Expression::PrivateFieldExpression(x), Expression::PrivateFieldExpression(y)) => {
            x.field.name == y.field.name && same_reference(ctx, &x.object, &y.object)
        }
        (Expression::ComputedMemberExpression(x), Expression::ComputedMemberExpression(y)) => {
            same_reference(ctx, &x.object, &y.object)
                && ctx.same_tokens(x.expression.span(), y.expression.span())
        }
        _ => false,
    }
}

/// Initializer of a `const` binding referenced by `e` (identifier), never reassigned.
pub fn const_init<'b, 'a>(ctx: &'b Ctx<'a, '_>, e: &Expression<'_>) -> Option<&'b Expression<'a>> {
    let Expression::Identifier(id) = strip_parens(e) else {
        return None;
    };
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return None;
    };
    if !matches!(v.id, BindingPattern::BindingIdentifier(_)) {
        return None;
    }
    match ctx.parent_kind(decl.id()) {
        AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const => {}
        _ => return None,
    }
    v.init.as_ref().map(strip_parens)
}

/// Like eslint-utils `getStaticValue(...)` being a string: literals, constant templates,
/// concatenations and `const` bindings thereof.
pub fn is_static_string(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> bool {
    if depth > 4 {
        return false;
    }
    match strip_parens(e) {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(t) => t
            .expressions
            .iter()
            .all(|x| is_static_primitive(ctx, x, depth + 1)),
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            (is_static_string(ctx, &b.left, depth + 1)
                && is_static_primitive(ctx, &b.right, depth + 1))
                || (is_static_primitive(ctx, &b.left, depth + 1)
                    && is_static_string(ctx, &b.right, depth + 1))
        }
        e @ Expression::Identifier(_) => {
            const_init(ctx, e).is_some_and(|i| is_static_string(ctx, i, depth + 1))
        }
        _ => false,
    }
}

fn is_static_primitive(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> bool {
    match strip_parens(e) {
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        e => is_static_string(ctx, e, depth),
    }
}

/// Array literal, or `const` binding to an array literal of static primitives.
pub fn is_static_array(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ArrayExpression(_) => true,
        e @ Expression::Identifier(_) => {
            matches!(const_init(ctx, e), Some(Expression::ArrayExpression(a))
            if a.elements.iter().all(|el| el.as_expression().is_some_and(|x| is_static_primitive(ctx, x, 1))))
        }
        _ => false,
    }
}

/// Static string value of a literal, constant template or `const` binding to one.
pub fn static_str<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<&'a str> {
    match strip_parens(e) {
        e @ (Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) => {
            crate::helpers::ast::string_value(e)
        }
        e @ Expression::Identifier(_) => {
            let init = const_init(ctx, e)?;
            match init {
                Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                    crate::helpers::ast::string_value(init)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Non-computed object property with an identifier key `name` (ESTree `Property`).
pub fn find_property<'b, 'a>(
    obj: &'b ObjectExpression<'a>,
    name: &str,
) -> Option<&'b ObjectProperty<'a>> {
    obj.properties.iter().rev().find_map(|p| match p {
        ObjectPropertyKind::ObjectProperty(p)
            if !p.computed
                && matches!(&p.key, PropertyKey::StaticIdentifier(id) if id.name == name) =>
        {
            Some(&**p)
        }
        _ => None,
    })
}

/// Ancestors of a node, nearest (the parent) first.
///
/// Note: `Ctx::ancestors` skips the direct parent with oxc 0.154 (`ancestor_ids` already excludes
/// the node itself), so this batch uses its own walk.
pub fn ancestors<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: oxc_semantic::NodeId,
) -> impl Iterator<Item = &'s oxc_semantic::AstNode<'a>> + 's {
    ctx.nodes().ancestors(id)
}

/// Nearest enclosing function or arrow function.
pub fn enclosing_fn<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: oxc_semantic::NodeId,
) -> Option<&'s oxc_semantic::AstNode<'a>> {
    ancestors(ctx, id).find(|n| {
        matches!(
            n.kind(),
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
        )
    })
}

/// Parent skipping only parenthesized expressions (ESTree has no such nodes).
pub fn paren_parent<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: oxc_semantic::NodeId,
) -> &'s oxc_semantic::AstNode<'a> {
    let mut p = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = p.kind() {
        p = ctx.parent(p.id());
    }
    p
}

pub const TYPED_ARRAYS: &[&str] = &[
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

/// `Obj.method(...)` where `Obj` is an identifier in `objects` and `method` in `methods`
/// (non-optional, optional exact argument count, no spread).
pub fn is_static_call(
    call: &CallExpression<'_>,
    objects: &[&str],
    methods: &[&str],
    args: Option<usize>,
) -> bool {
    is_method_call(call, methods, args, false).is_some_and(
        |m| matches!(&m.object, Expression::Identifier(o) if objects.contains(&o.name.as_str())),
    )
}

fn is_logical_and_or(k: &AstKind<'_>) -> bool {
    matches!(k, AstKind::LogicalExpression(l) if matches!(l.operator, LogicalOperator::And | LogicalOperator::Or))
}

fn is_boolean_call(k: &AstKind<'_>) -> bool {
    matches!(k, AstKind::CallExpression(c)
        if c.arguments.len() == 1 && matches!(&c.callee, Expression::Identifier(id) if id.name == "Boolean"))
}

/// Unicorn's `isBooleanExpression`: the value of the node is used as a boolean
/// (`!x`, `Boolean(x)`, or an operand of `&&`/`||` that itself is).
pub fn is_boolean_expression(ctx: &Ctx<'_, '_>, id: oxc_semantic::NodeId) -> bool {
    let node = ctx.node(id);
    let kind = node.kind();
    if matches!(kind, AstKind::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot)
        || is_boolean_call(&kind)
    {
        return true;
    }
    let parent = paren_parent(ctx, id);
    let pk = parent.kind();
    if matches!(pk, AstKind::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot) {
        return true;
    }
    if let AstKind::CallExpression(c) = pk
        && is_boolean_call(&pk)
        && c.arguments[0]
            .as_expression()
            .is_some_and(|a| strip_parens(a).span() == kind.span())
    {
        return true;
    }
    if is_logical_and_or(&pk) {
        return is_boolean_expression(ctx, parent.id());
    }
    false
}

/// Unicorn's `isControlFlowTest`: the node is (an operand of `&&`/`||` that is) the test of an
/// `if`, `?:`, `while`, `do…while` or `for`.
pub fn is_control_flow_test(ctx: &Ctx<'_, '_>, id: oxc_semantic::NodeId) -> bool {
    let span = ctx.node(id).kind().span();
    let parent = paren_parent(ctx, id);
    let is_test = |t: &Expression<'_>| strip_parens(t).span() == span;
    match parent.kind() {
        AstKind::IfStatement(s) => is_test(&s.test),
        AstKind::ConditionalExpression(c) => is_test(&c.test),
        AstKind::WhileStatement(s) => is_test(&s.test),
        AstKind::DoWhileStatement(s) => is_test(&s.test),
        AstKind::ForStatement(s) => s.test.as_ref().is_some_and(is_test),
        k if is_logical_and_or(&k) => is_control_flow_test(ctx, parent.id()),
        _ => false,
    }
}

/// `-1` literal.
pub fn is_negative_one(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::UnaryExpression(u)
        if u.operator == UnaryOperator::UnaryNegation
            && matches!(strip_parens(&u.argument), Expression::NumericLiteral(n) if n.value == 1.0))
}

/// `0` literal.
pub fn is_zero(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.value == 0.0)
}

fn single_return<'b, 'a>(body: &'b FunctionBody<'a>) -> Option<&'b Expression<'a>> {
    match body.statements.as_slice() {
        [Statement::ReturnStatement(r)] => r.argument.as_ref(),
        _ => None,
    }
}

/// `array.<method>(x => x === value)` where the callback's only parameter is used once, in the
/// comparison: returns the method property span. Shared by `prefer-array-index-of` and
/// `prefer-includes` (unicorn's `simple-array-search-rule`).
pub fn simple_array_search(
    ctx: &Ctx<'_, '_>,
    call: &CallExpression<'_>,
    method: &str,
) -> Option<Span> {
    let m = is_method_call(call, &[method], Some(1), true)?;
    if call.optional {
        return None;
    }
    let cb = arg(&call.arguments, 0)?;
    let (params, body_expr) = match cb {
        Expression::ArrowFunctionExpression(a) if !a.r#async => {
            let e = match &a.body {
                ArrowFunctionBody::FunctionBody(b) => single_return(b)?,
                body => body.as_expression()?,
            };
            (&a.params, e)
        }
        Expression::FunctionExpression(f) if !f.r#async && !f.generator => {
            // `this`, `arguments` or the function's own name used inside.
            if f.id.is_some()
                || ctx.tokens_in(f.span).iter().any(|t| {
                    t.kind() == oxc_parser::Kind::This
                        || (t.kind().is_identifier() && ctx.text(t.span()) == "arguments")
                })
            {
                return None;
            }
            (&f.params, single_return(f.body.as_ref()?)?)
        }
        _ => return None,
    };
    if params.items.len() != 1 || params.rest.is_some() || params.items[0].initializer.is_some() {
        return None;
    }
    let BindingPattern::BindingIdentifier(param) = &params.items[0].pattern else {
        return None;
    };
    let Expression::BinaryExpression(b) = strip_parens(body_expr) else {
        return None;
    };
    if b.operator != BinaryOperator::StrictEquality {
        return None;
    }
    let left_is = matches!(&b.left, Expression::Identifier(id) if id.name == param.name);
    let right_is = matches!(&b.right, Expression::Identifier(id) if id.name == param.name);
    if !left_is && !right_is {
        return None;
    }
    let sym = param.symbol_id.get()?;
    if ctx.scoping().get_resolved_reference_ids(sym).len() != 1 {
        return None;
    }
    Some(m.property.span)
}
