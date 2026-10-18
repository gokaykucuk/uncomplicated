//! JSX list components should not use array indexes as key
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Use a stable ID as key, because the index changes when the list changes.";

fn index_position(method: &str) -> Option<usize> {
    match method {
        "every" | "filter" | "find" | "findIndex" | "flatMap" | "forEach" | "map" | "some" => {
            Some(1)
        }
        "reduce" | "reduceRight" => Some(2),
        _ => None,
    }
}

fn callee_member<'b, 'a>(call: &'b CallExpression<'a>) -> Option<&'b StaticMemberExpression<'a>> {
    match &call.callee {
        Expression::StaticMemberExpression(m) => Some(m),
        _ => None,
    }
}

fn is_react_children(m: &StaticMemberExpression<'_>) -> bool {
    if !matches!(m.property.name.as_str(), "map" | "forEach") {
        return false;
    }
    match &m.object {
        Expression::Identifier(o) => o.name == "Children",
        Expression::StaticMemberExpression(o) => {
            matches!(&o.object, Expression::Identifier(r) if r.name == "React")
        }
        _ => false,
    }
}

fn param_name<'a>(params: &FormalParameters<'a>, pos: usize) -> Option<&'a str> {
    match &params.items.get(pos)?.pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn callback_params<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b FormalParameters<'a>> {
    match e {
        Expression::ArrowFunctionExpression(a) => Some(&a.params),
        Expression::FunctionExpression(f) => Some(&f.params),
        _ => None,
    }
}

/// `getMapIndexParamName` of upstream: the index parameter name of an iterator call.
fn map_index_name<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    let m = callee_member(call)?;
    let pos = index_position(&m.property.name)?;
    let cb = if is_react_children(m) {
        call.arguments.get(1)
    } else {
        call.arguments.first()
    };
    param_name(callback_params(cb?.as_expression()?)?, pos)
}

/// Is `name` the index parameter of an iterator call enclosing `id`?
fn is_index(ctx: &Ctx<'_, '_>, id: NodeId, name: &str) -> bool {
    ancestors(ctx, id)
        .any(|a| matches!(a.kind(), AstKind::CallExpression(c) if map_index_name(c) == Some(name)))
}

fn is_static_array(e: &Expression<'_>) -> bool {
    let Expression::ArrayExpression(a) = e else {
        return false;
    };
    a.elements.iter().all(|el| match el {
        ArrayExpressionElement::Elision(_) => true,
        ArrayExpressionElement::SpreadElement(_) => false,
        ArrayExpressionElement::StringLiteral(_)
        | ArrayExpressionElement::NumericLiteral(_)
        | ArrayExpressionElement::BooleanLiteral(_)
        | ArrayExpressionElement::NullLiteral(_)
        | ArrayExpressionElement::BigIntLiteral(_)
        | ArrayExpressionElement::RegExpLiteral(_) => true,
        ArrayExpressionElement::TemplateLiteral(t) => t.expressions.is_empty(),
        e => e
            .as_expression()
            .is_some_and(|e| matches!(e, Expression::ArrayExpression(_)) && is_static_array(e)),
    })
}

fn is_array_from_length(e: &Expression<'_>) -> bool {
    let Expression::CallExpression(c) = e else {
        return false;
    };
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return false;
    };
    if m.property.name != "from"
        || !matches!(&m.object, Expression::Identifier(o) if o.name == "Array")
    {
        return false;
    }
    if c.arguments.len() != 1 {
        return false;
    }
    let Some(Expression::ObjectExpression(o)) = c.arguments[0].as_expression().map(strip_ts) else {
        return false;
    };
    o.properties.iter().any(|p| match p {
        ObjectPropertyKind::ObjectProperty(p) => {
            !p.computed
                && property_key_name(&p.key) == Some("length")
                && matches!(p.value, Expression::NumericLiteral(_))
        }
        _ => false,
    })
}

const WRITING: &[&str] = &[
    "copyWithin",
    "fill",
    "pop",
    "push",
    "reverse",
    "set",
    "shift",
    "sort",
    "splice",
    "unshift",
    "add",
    "clear",
    "delete",
];

fn is_mutated(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    ctx.scoping().get_resolved_references(sym).any(|r| {
        if r.is_write() {
            return true;
        }
        let parent = ctx.parent(r.node_id());
        match parent.kind() {
            AstKind::StaticMemberExpression(_) | AstKind::ComputedMemberExpression(_) => {
                let pspan = parent.kind().span();
                match ctx.parent_kind(parent.id()) {
                    AstKind::AssignmentExpression(a) => a.left.span() == pspan,
                    AstKind::UpdateExpression(_) => true,
                    AstKind::CallExpression(c) => {
                        c.callee.span() == pspan
                            && matches!(parent.kind(), AstKind::StaticMemberExpression(m) if WRITING.contains(&m.property.name.as_str()))
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    })
}

fn resolves_to_static_array(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> bool {
    let e = strip_ts(e);
    if is_static_array(e) {
        return true;
    }
    let Expression::Identifier(id) = e else {
        return false;
    };
    if depth > 4 {
        return false;
    }
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return false;
    };
    let is_const = matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const);
    is_const
        && matches!(v.id, BindingPattern::BindingIdentifier(_))
        && !is_mutated(ctx, sym)
        && v.init
            .as_ref()
            .is_some_and(|i| resolves_to_static_array(ctx, i, depth + 1))
}

/// Sonar: the owning iterator call iterates a statically known list.
fn is_static_list_key(ctx: &Ctx<'_, '_>, id: NodeId, name: &str) -> bool {
    for a in ancestors(ctx, id) {
        let params = match a.kind() {
            AstKind::ArrowFunctionExpression(f) => &f.params,
            AstKind::Function(f) if f.is_expression() => &f.params,
            _ => continue,
        };
        let AstKind::CallExpression(call) = ctx.parent_kind(a.id()) else {
            continue;
        };
        let Some(m) = callee_member(call) else {
            continue;
        };
        let Some(pos) = index_position(&m.property.name) else {
            continue;
        };
        if call.arguments.first().map(GetSpan::span) != Some(a.kind().span()) {
            continue;
        }
        if param_name(params, pos) != Some(name) {
            continue;
        }
        let src = &m.object;
        return is_static_array(strip_ts(src))
            || is_array_from_length(strip_ts(src))
            || resolves_to_static_array(ctx, src, 0);
    }
    false
}

fn check_value(ctx: &Ctx<'_, '_>, id: NodeId, value: &Expression<'_>) {
    let report = |ident: &IdentifierReference<'_>, span: Span| {
        if is_index(ctx, id, &ident.name) && !is_static_list_key(ctx, id, &ident.name) {
            ctx.report(span, MSG);
        }
    };
    match strip_parens(value) {
        Expression::Identifier(i) => report(i, i.span),
        Expression::TemplateLiteral(t) if t.expressions.len() == 1 => {
            if let Expression::Identifier(i) = &t.expressions[0] {
                report(i, t.span);
            }
        }
        Expression::CallExpression(c) => match &c.callee {
            Expression::StaticMemberExpression(m) if m.property.name == "toString" => {
                if let Expression::Identifier(i) = &m.object {
                    report(i, c.span);
                }
            }
            Expression::Identifier(f) if f.name == "String" => {
                if let Some(Expression::Identifier(i)) =
                    c.arguments.first().and_then(Argument::as_expression)
                {
                    report(i, i.span);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn is_create_clone_element(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    match callee {
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(o) if o.name == "React")
                && matches!(m.property.name.as_str(), "createElement" | "cloneElement")
        }
        Expression::Identifier(_) => fqn(ctx, callee)
            .is_some_and(|f| f == "react.createElement" || f == "react.cloneElement"),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::JSXAttribute(a) => {
                if attr_name(a) != Some("key") {
                    return;
                }
                if let Some(e) = attr_expr(a) {
                    check_value(ctx, node.id(), e);
                }
            }
            AstKind::CallExpression(c) => {
                if c.arguments.len() < 2 || !is_create_clone_element(ctx, &c.callee) {
                    return;
                }
                let Some(Expression::ObjectExpression(o)) = c.arguments[1].as_expression() else {
                    return;
                };
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p
                        && matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "key")
                    {
                        check_value(ctx, node.id(), &p.value);
                    }
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
    fn index_keys() {
        check(
            "S6479",
            "a.jsx",
            r#"
const a = things.map((thing, index) => <Hello key={index} />); // Noncompliant {{Use a stable ID as key, because the index changes when the list changes.}}
const b = things.map((thing, index) => <Hello key={`foo-${index}`} />); // Noncompliant
const c = things.map((thing, index) => <Hello key={index.toString()} />); // Noncompliant
const d = things.map((thing, index) => <Hello key={String(index)} />); // Noncompliant
const e = things.reduce((acc, thing, i) => acc.concat(<Hello key={i} />), []); // Noncompliant
const f = things.map((thing, index) => <Hello key={'foo' + index} />);
const g = things.map((thing, index) => <Hello key={`${thing.id}-${index}`} />);
const h = things.map((thing) => <Hello key={thing.id} />);
const i = ['a', 'b'].map((x, index) => <Hello key={index} />);
const STATIC = [1, 2, 3];
const j = STATIC.map((x, index) => <Hello key={index} />);
const k = Array.from({ length: 3 }).map((_, index) => <Hello key={index} />);
const MUT = [1, 2];
MUT.push(3);
const l = MUT.map((x, index) => <Hello key={index} />); // Noncompliant
const m = things.map((thing, index) => React.cloneElement(thing, { key: index })); // Noncompliant
const n = React.Children.map(children, (child, index) => <Hello key={index} />); // Noncompliant
"#,
        );
    }
}
