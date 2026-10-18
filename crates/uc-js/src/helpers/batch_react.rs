//! Helpers owned by the `react` rule batch.
//!
//! Most of these mirror the component / hook detection heuristics of `eslint-plugin-react`
//! (`Components.detect`, `jsxUtil`, `componentUtil`) and `eslint-plugin-react-hooks`, adapted to
//! the oxc AST (where parentheses are explicit nodes and arrow expression bodies have the arrow
//! as direct parent).
use crate::prelude::*;

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// `eslint-plugin-react`'s `isFirstLetterCapitalized`: leading underscores are ignored and any
/// character equal to its upper-case form (including non-letters) counts as capitalized.
pub fn is_first_letter_capitalized(word: &str) -> bool {
    let rest = word.trim_start_matches('_');
    match rest.chars().next() {
        None => !word.is_empty(),
        Some(c) => c.to_uppercase().eq(std::iter::once(c)),
    }
}

/// React hook name: `use` or `use` followed by an upper-case letter or a digit.
pub fn is_hook_name(name: &str) -> bool {
    name == "use"
        || (name.len() > 3
            && name.starts_with("use")
            && (name.as_bytes()[3].is_ascii_uppercase() || name.as_bytes()[3].is_ascii_digit()))
}

/// `rules-of-hooks` component name: starts with an upper-case ASCII letter.
pub fn is_component_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

// ---------------------------------------------------------------------------------------------
// JSX
// ---------------------------------------------------------------------------------------------

/// Source text of a JSX element name (`div`, `Foo.Bar`, `svg:rect`).
pub fn jsx_name_text<'a>(ctx: &Ctx<'a, '_>, name: &JSXElementName<'a>) -> &'a str {
    ctx.text(name.span())
}

/// Plain identifier name of a JSX element (`div`, `Foo`), `None` for member / namespaced names.
pub fn jsx_ident_name<'a>(name: &JSXElementName<'a>) -> Option<&'a str> {
    match name {
        JSXElementName::Identifier(id) => Some(id.name.as_str()),
        JSXElementName::IdentifierReference(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// `jsxUtil.isDOMComponent`: the element name starts with a lower-case ASCII letter.
pub fn is_dom_component<'a>(ctx: &Ctx<'a, '_>, el: &JSXOpeningElement<'a>) -> bool {
    jsx_name_text(ctx, &el.name)
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
}

/// Name of a non-namespaced JSX attribute.
pub fn attr_name<'a>(attr: &JSXAttribute<'a>) -> Option<&'a str> {
    match &attr.name {
        JSXAttributeName::Identifier(id) => Some(id.name.as_str()),
        JSXAttributeName::NamespacedName(_) => None,
    }
}

/// First attribute called `name` (case-sensitive).
pub fn find_attr<'b, 'a>(
    el: &'b JSXOpeningElement<'a>,
    name: &str,
) -> Option<&'b JSXAttribute<'a>> {
    el.attributes.iter().find_map(|a| match a {
        JSXAttributeItem::Attribute(a) if attr_name(a) == Some(name) => Some(&**a),
        _ => None,
    })
}

/// First attribute called `name`, ignoring ASCII case (`jsx-ast-utils` `getProp` default).
pub fn find_attr_ignore_case<'b, 'a>(
    el: &'b JSXOpeningElement<'a>,
    name: &str,
) -> Option<&'b JSXAttribute<'a>> {
    el.attributes.iter().find_map(|a| match a {
        JSXAttributeItem::Attribute(a)
            if attr_name(a).is_some_and(|n| n.eq_ignore_ascii_case(name)) =>
        {
            Some(&**a)
        }
        _ => None,
    })
}

/// Expression of an attribute value written as `{expr}` (not `{}`).
pub fn attr_expr<'b, 'a>(attr: &'b JSXAttribute<'a>) -> Option<&'b Expression<'a>> {
    match &attr.value {
        Some(JSXAttributeValue::ExpressionContainer(c)) => c.expression.as_expression(),
        _ => None,
    }
}

/// True if the element has a spread attribute.
pub fn has_spread_attr(el: &JSXOpeningElement<'_>) -> bool {
    el.attributes
        .iter()
        .any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)))
}

/// True for `<div>` / `<>` (after parentheses).
pub fn is_jsx(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
}

// ---------------------------------------------------------------------------------------------
// Tree navigation
// ---------------------------------------------------------------------------------------------

/// Parent node as ESTree would see it: parentheses are skipped.
pub fn estree_parent<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut cur = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = cur.kind() {
        if cur.id() == NodeId::ROOT {
            break;
        }
        cur = ctx.parent(cur.id());
    }
    cur
}

/// Ancestors of a node, nearest (the parent) first. (`Ctx::ancestors` skips the parent.)
pub fn ancestors<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
}

/// The node itself, then its ancestors (ESLint's `getScope(node)` starts at the node's own scope
/// for functions and classes).
pub fn self_and_ancestors<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
    std::iter::once(ctx.node(id)).chain(ancestors(ctx, id))
}

/// Nearest function-like ancestor (function or arrow), not crossing anything.
pub fn enclosing_function<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> Option<&'s AstNode<'a>> {
    ancestors(ctx, id).find(|n| is_function(&n.kind()))
}

/// Body statements of a function-like node (`None` for expression arrows / bodiless functions).
pub fn function_statements<'a>(kind: &AstKind<'a>) -> Option<&'a [Statement<'a>]> {
    match kind {
        AstKind::Function(f) => f.body.as_ref().map(|b| b.statements.as_slice()),
        AstKind::ArrowFunctionExpression(a) => {
            a.body.as_function_body().map(|b| b.statements.as_slice())
        }
        _ => None,
    }
}

/// Calls `f` with every `return` statement of `stmts`, not descending into nested functions.
/// Stops early when `f` returns `true`; returns whether it did.
pub fn any_return<'a>(
    stmts: &[Statement<'a>],
    f: &mut dyn FnMut(&ReturnStatement<'a>) -> bool,
) -> bool {
    stmts.iter().any(|s| stmt_any_return(s, f))
}

fn stmt_any_return<'a>(s: &Statement<'a>, f: &mut dyn FnMut(&ReturnStatement<'a>) -> bool) -> bool {
    match s {
        Statement::ReturnStatement(r) => f(r),
        Statement::BlockStatement(b) => any_return(&b.body, f),
        Statement::IfStatement(i) => {
            stmt_any_return(&i.consequent, f)
                || i.alternate.as_ref().is_some_and(|a| stmt_any_return(a, f))
        }
        Statement::ForStatement(x) => stmt_any_return(&x.body, f),
        Statement::ForInStatement(x) => stmt_any_return(&x.body, f),
        Statement::ForOfStatement(x) => stmt_any_return(&x.body, f),
        Statement::WhileStatement(x) => stmt_any_return(&x.body, f),
        Statement::DoWhileStatement(x) => stmt_any_return(&x.body, f),
        Statement::LabeledStatement(x) => stmt_any_return(&x.body, f),
        Statement::WithStatement(x) => stmt_any_return(&x.body, f),
        Statement::SwitchStatement(x) => x.cases.iter().any(|c| any_return(&c.consequent, f)),
        Statement::TryStatement(t) => {
            any_return(&t.block.body, f)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| any_return(&h.body.body, f))
                || t.finalizer.as_ref().is_some_and(|b| any_return(&b.body, f))
        }
        _ => false,
    }
}

/// Calls `f` for each returned value of a function-like node (expression body or `return x`).
/// Stops when `f` returns true.
pub fn any_returned_value<'a>(
    kind: &AstKind<'a>,
    f: &mut dyn FnMut(&Expression<'a>) -> bool,
) -> bool {
    if let AstKind::ArrowFunctionExpression(a) = kind
        && let Some(e) = a.get_expression()
    {
        return f(e);
    }
    match function_statements(kind) {
        Some(stmts) => stmts.iter().any(|s| react_stmt_returns(s, f)),
        None => false,
    }
}

/// `astUtil.traverseReturns`: only descends into blocks, `if`, `for`, `while` and `switch`.
fn react_stmt_returns<'a>(s: &Statement<'a>, f: &mut dyn FnMut(&Expression<'a>) -> bool) -> bool {
    match s {
        Statement::ReturnStatement(r) => r.argument.as_ref().is_some_and(&mut *f),
        Statement::BlockStatement(b) => b.body.iter().any(|s| react_stmt_returns(s, f)),
        Statement::IfStatement(i) => {
            react_stmt_returns(&i.consequent, f)
                || i.alternate
                    .as_ref()
                    .is_some_and(|a| react_stmt_returns(a, f))
        }
        Statement::ForStatement(x) => react_stmt_returns(&x.body, f),
        Statement::WhileStatement(x) => react_stmt_returns(&x.body, f),
        Statement::SwitchStatement(x) => x
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| react_stmt_returns(s, f))),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

/// `jsxUtil.isReturningJSX` value check.
fn is_jsx_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>, strict: bool, ignore_null: bool) -> bool {
    match strip_parens(e) {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ConditionalExpression(c) => {
            let (a, b) = (
                is_jsx_value(ctx, &c.consequent, strict, ignore_null),
                is_jsx_value(ctx, &c.alternate, strict, ignore_null),
            );
            if strict { a && b } else { a || b }
        }
        Expression::LogicalExpression(l) => {
            let (a, b) = (
                is_jsx_value(ctx, &l.left, strict, ignore_null),
                is_jsx_value(ctx, &l.right, strict, ignore_null),
            );
            if strict { a && b } else { a || b }
        }
        Expression::SequenceExpression(s) => s
            .expressions
            .last()
            .is_some_and(|e| is_jsx_value(ctx, e, strict, ignore_null)),
        Expression::NullLiteral(_) => !ignore_null,
        Expression::CallExpression(c) => is_create_element(ctx, c),
        Expression::Identifier(id) => {
            ctx.symbol_of(id)
                .is_some_and(|s| match ctx.symbol_decl(s).kind() {
                    AstKind::VariableDeclarator(v) => v.init.as_ref().is_some_and(|i| {
                        matches!(i, Expression::JSXElement(_) | Expression::JSXFragment(_))
                    }),
                    _ => false,
                })
        }
        _ => false,
    }
}

/// `utils.isReturningJSX` (null not counted).
pub fn returns_jsx(ctx: &Ctx<'_, '_>, kind: &AstKind<'_>) -> bool {
    any_returned_value(kind, &mut |e| is_jsx_value(ctx, e, false, true))
}

/// `utils.isReturningJSXOrNull`.
pub fn returns_jsx_or_null(ctx: &Ctx<'_, '_>, kind: &AstKind<'_>) -> bool {
    any_returned_value(kind, &mut |e| is_jsx_value(ctx, e, false, false))
}

/// `utils.isReturningOnlyNull`.
pub fn returns_only_null(kind: &AstKind<'_>) -> bool {
    let mut found_null = false;
    let mut other = false;
    any_returned_value(kind, &mut |e| {
        if matches!(strip_parens(e), Expression::NullLiteral(_)) {
            found_null = true;
        } else {
            other = true;
        }
        false
    });
    found_null && !other
}

/// `React.createElement(...)` or `createElement(...)` imported from React.
pub fn is_create_element(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m) => {
            m.property.name == "createElement"
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        Expression::Identifier(id) => {
            id.name == "createElement"
                && fqn(ctx, &call.callee).as_deref() == Some("react.createElement")
        }
        _ => false,
    }
}

fn binding_name<'a>(p: &BindingPattern<'a>) -> Option<&'a str> {
    match p {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn assign_left_ident<'a>(a: &AssignmentExpression<'a>) -> Option<&'a str> {
    match &a.left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn is_member_target(a: &AssignmentExpression<'_>) -> bool {
    a.left.as_member_expression().is_some()
}

fn is_module_exports_target(a: &AssignmentExpression<'_>) -> bool {
    match a.left.as_member_expression() {
        Some(MemberExpression::StaticMemberExpression(m)) => {
            m.property.name == "exports"
                && matches!(&m.object, Expression::Identifier(o) if o.name == "module")
        }
        _ => false,
    }
}

fn key_ident<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// `utils.isPragmaComponentWrapper`: `React.memo(...)` / `React.forwardRef(...)` (unless it just
/// wraps an already detected component), or `memo` / `forwardRef` imported from React.
pub fn is_pragma_component_wrapper(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match &call.callee {
        Expression::Identifier(id) => {
            matches!(id.name.as_str(), "memo" | "forwardRef")
                && fqn(ctx, &call.callee).is_some_and(|f| f == format!("react.{}", id.name))
        }
        Expression::StaticMemberExpression(m) => {
            matches!(m.property.name.as_str(), "memo" | "forwardRef")
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
                && !wraps_detected_component(ctx, call)
        }
        _ => false,
    }
}

/// `utils.nodeWrapsComponent`: the wrapped function directly renders a component declared
/// earlier as a class component or an arrow assigned to a variable.
fn wraps_detected_component(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    let Some(first) = call.arguments.first().and_then(Argument::as_expression) else {
        return false;
    };
    let body = match first {
        Expression::ArrowFunctionExpression(a) => match a.get_expression() {
            Some(e) => Some(e),
            None => a.get_function_body().and_then(|b| {
                b.statements.iter().find_map(|s| match s {
                    Statement::ReturnStatement(r) => r.argument.as_ref(),
                    _ => None,
                })
            }),
        },
        Expression::FunctionExpression(f) => f.body.as_ref().and_then(|b| {
            b.statements.iter().find_map(|s| match s {
                Statement::ReturnStatement(r) => r.argument.as_ref(),
                _ => None,
            })
        }),
        _ => None,
    };
    let Some(Expression::JSXElement(el)) = body else {
        return false;
    };
    let JSXElementName::IdentifierReference(id) = &el.opening_element.name else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let decl = ctx.symbol_decl(sym);
    if decl.kind().span().start > call.span.start {
        return false;
    }
    match decl.kind() {
        AstKind::Class(c) => is_es6_component(c),
        AstKind::VariableDeclarator(v) => match v.init.as_ref() {
            Some(Expression::ArrowFunctionExpression(a)) => {
                stateless_component(ctx, ctx.node(a.node_id.get())) == Some(a.node_id.get())
            }
            Some(Expression::ClassExpression(c)) => is_es6_component(c),
            _ => false,
        },
        _ => false,
    }
}

/// The arrow whose expression body is the node `id`, if any.
fn expression_body_of<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId, parent: &'s AstNode<'a>) -> bool {
    if let AstKind::ArrowFunctionExpression(a) = parent.kind() {
        return a.get_expression().is_some_and(|e| {
            let me = ctx.node(id).kind().span();
            strip_parens(e).span() == me || e.span() == me
        });
    }
    false
}

fn in_allowed_position(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let parent = estree_parent(ctx, id);
    match parent.kind() {
        AstKind::VariableDeclarator(_)
        | AstKind::AssignmentExpression(_)
        | AstKind::ObjectProperty(_)
        | AstKind::ReturnStatement(_)
        | AstKind::ExportDefaultDeclaration(_) => true,
        AstKind::ArrowFunctionExpression(_) => expression_body_of(ctx, id, parent),
        AstKind::SequenceExpression(s) => {
            let me = ctx.node(id).kind().span();
            s.expressions
                .last()
                .is_some_and(|l| strip_parens(l).span() == me || l.span() == me)
                && in_allowed_position(ctx, parent.id())
        }
        _ => false,
    }
}

/// `Components.detect`'s `utils.getStatelessComponent`: the component node for a function-like
/// node (the function itself, or the outermost `memo` / `forwardRef` call wrapping it).
pub fn stateless_component(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> Option<NodeId> {
    let kind = node.kind();
    match kind {
        AstKind::Function(f) if f.is_function_declaration() => {
            let ok = !(f.r#async && f.generator)
                && f.id
                    .as_ref()
                    .is_none_or(|id| is_first_letter_capitalized(&id.name))
                && returns_jsx_or_null(ctx, &kind);
            ok.then(|| node.id())
        }
        AstKind::Function(f) if f.is_expression() => {
            if f.r#async && f.generator {
                return None;
            }
            expression_component(ctx, node, f.id.as_ref().map(|i| i.name.as_str()))
        }
        AstKind::ArrowFunctionExpression(_) => expression_component(ctx, node, None),
        _ => None,
    }
}

/// True if the function-like node is itself a detected function component.
pub fn is_stateless_component(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    stateless_component(ctx, node) == Some(node.id())
}

/// `components.get(node)`: is this node a detected component (function, wrapper call, class,
/// ES5 object)?
pub fn is_component_node(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    match node.kind() {
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
            is_stateless_component(ctx, node)
        }
        AstKind::CallExpression(c) => {
            is_pragma_component_wrapper(ctx, c)
                && c.arguments
                    .first()
                    .and_then(Argument::as_expression)
                    .is_some_and(|a| {
                        matches!(
                            a,
                            Expression::ArrowFunctionExpression(_)
                                | Expression::FunctionExpression(_)
                        )
                    })
        }
        AstKind::Class(c) => is_es6_component(c),
        AstKind::ObjectExpression(_) => is_es5_component(ctx, node.id()),
        _ => false,
    }
}

fn expression_component(
    ctx: &Ctx<'_, '_>,
    node: &AstNode<'_>,
    own_name: Option<&str>,
) -> Option<NodeId> {
    let yes = Some(node.id());
    let kind = node.kind();
    let id = node.id();
    let parent = estree_parent(ctx, id);
    let pkind = parent.kind();
    let is_property_assignment =
        matches!(pkind, AstKind::AssignmentExpression(a) if is_member_target(a));
    let is_module_exports =
        matches!(pkind, AstKind::AssignmentExpression(a) if is_module_exports_target(a));
    let parent_is_arrow = expression_body_of(ctx, id, parent);
    let jsx_or_null = || returns_jsx_or_null(ctx, &kind);
    let cap = |n: Option<&str>| n.is_some_and(is_first_letter_capitalized);

    if let AstKind::ExportDefaultDeclaration(_) = pkind {
        return returns_jsx(ctx, &kind).then_some(node.id());
    }
    if let AstKind::VariableDeclarator(v) = pkind
        && jsx_or_null()
    {
        return cap(binding_name(&v.id)).then_some(node.id());
    }
    if (matches!(pkind, AstKind::ReturnStatement(_)) || parent_is_arrow) && !returns_jsx(ctx, &kind)
    {
        return None;
    }
    if let AstKind::AssignmentExpression(a) = pkind
        && !is_property_assignment
        && jsx_or_null()
    {
        return cap(assign_left_ident(a)).then_some(node.id());
    }
    if parent_is_arrow {
        let grand = estree_parent(ctx, parent.id());
        match grand.kind() {
            AstKind::AssignmentExpression(a) if jsx_or_null() => {
                return cap(assign_left_ident(a)).then_some(node.id());
            }
            AstKind::ObjectProperty(p) if jsx_or_null() => {
                return cap(key_ident(&p.key)).then_some(node.id());
            }
            _ => {}
        }
    }
    if let AstKind::ReturnStatement(_) = pkind {
        if cap(own_name) {
            return yes;
        }
        let p1 = ctx.parent(parent.id());
        let function_expr = ctx.parent(p1.id());
        match estree_parent(ctx, function_expr.id()).kind() {
            AstKind::AssignmentExpression(a) if jsx_or_null() => {
                return cap(assign_left_ident(a)).then_some(node.id());
            }
            AstKind::ObjectProperty(p) if jsx_or_null() => {
                return cap(key_ident(&p.key)).then_some(node.id());
            }
            _ => {}
        }
    }
    let parent_member_key = match pkind {
        AstKind::ObjectProperty(p) => p.computed && is_member_key(&p.key),
        AstKind::PropertyDefinition(p) => p.computed && is_member_key(&p.key),
        AstKind::MethodDefinition(p) => p.computed && is_member_key(&p.key),
        _ => false,
    };
    if parent_member_key && !returns_jsx(ctx, &kind) && !returns_only_null(&kind) {
        return None;
    }
    if let AstKind::ObjectProperty(p) = pkind
        && ((p.method && !p.computed) || (own_name.is_none() && !p.computed))
    {
        return (cap(key_ident(&p.key)) && returns_jsx(ctx, &kind)).then_some(node.id());
    }
    // `React.memo(() => <div />)`: the (outermost) wrapper call is the component.
    let mut wrapper = None;
    let mut cur = parent;
    while let AstKind::CallExpression(c) = cur.kind() {
        if !is_pragma_component_wrapper(ctx, c) {
            break;
        }
        wrapper = Some(cur.id());
        cur = estree_parent(ctx, cur.id());
    }
    if wrapper.is_some() && jsx_or_null() {
        return wrapper;
    }
    if !(in_allowed_position(ctx, id) && jsx_or_null()) {
        return None;
    }
    // isParentComponentNotStatelessComponent
    let parent_key = match pkind {
        AstKind::ObjectProperty(p) => key_ident(&p.key),
        AstKind::PropertyDefinition(p) => key_ident(&p.key),
        AstKind::MethodDefinition(p) => key_ident(&p.key),
        _ => None,
    };
    let has_params = match kind {
        AstKind::Function(f) => !f.params.items.is_empty() || f.params.rest.is_some(),
        AstKind::ArrowFunctionExpression(a) => {
            !a.params.items.is_empty() || a.params.rest.is_some()
        }
        _ => false,
    };
    if parent_key.is_some_and(|k| {
        k.chars()
            .next()
            .is_some_and(|c| c.to_lowercase().eq(std::iter::once(c)))
    }) && has_params
    {
        return None;
    }
    if let Some(n) = own_name {
        return is_first_letter_capitalized(n).then_some(node.id());
    }
    if is_property_assignment
        && !is_module_exports
        && let AstKind::AssignmentExpression(a) = pkind
        && !match a.left.as_member_expression() {
            Some(MemberExpression::StaticMemberExpression(m)) => {
                is_first_letter_capitalized(&m.property.name)
            }
            _ => false,
        }
    {
        return None;
    }
    if matches!(pkind, AstKind::ObjectProperty(_)) && returns_only_null(&kind) {
        return None;
    }
    yes
}

fn is_member_key(k: &PropertyKey<'_>) -> bool {
    matches!(
        k,
        PropertyKey::StaticMemberExpression(_) | PropertyKey::ComputedMemberExpression(_)
    )
}

/// `componentUtil.isES6Component`: `class X extends (React.)?(Pure)?Component`.
pub fn is_es6_component(class: &Class<'_>) -> bool {
    class
        .heritage
        .as_ref()
        .is_some_and(|h| is_react_super(&h.expression))
}

fn is_react_super(e: &Expression<'_>) -> bool {
    let ok = |n: &str| n == "Component" || n == "PureComponent";
    match strip_parens(e) {
        Expression::Identifier(id) => ok(&id.name),
        Expression::StaticMemberExpression(m) => {
            ok(&m.property.name)
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        _ => false,
    }
}

/// `class X extends (React.)?PureComponent`.
pub fn is_pure_component(class: &Class<'_>) -> bool {
    class
        .heritage
        .as_ref()
        .is_some_and(|h| match strip_parens(&h.expression) {
            Expression::Identifier(id) => id.name == "PureComponent",
            Expression::StaticMemberExpression(m) => {
                m.property.name == "PureComponent"
                    && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
            }
            _ => false,
        })
}

/// `componentUtil.isES5Component`: an object literal passed to `createReactClass(...)`.
pub fn is_es5_component(ctx: &Ctx<'_, '_>, obj_id: NodeId) -> bool {
    match estree_parent(ctx, obj_id).kind() {
        AstKind::CallExpression(c) => {
            let callee = ctx.text(c.callee.span());
            callee == "createReactClass" || callee == "React.createReactClass"
        }
        _ => false,
    }
}

/// Nearest enclosing class, if it is an ES6 React component.
pub fn parent_es6_component<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> Option<&'s AstNode<'a>> {
    let class = self_and_ancestors(ctx, id).find(|n| matches!(n.kind(), AstKind::Class(_)))?;
    match class.kind() {
        AstKind::Class(c) if is_es6_component(c) => Some(class),
        _ => None,
    }
}

/// Nearest enclosing `createReactClass({...})` object.
pub fn parent_es5_component<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> Option<&'s AstNode<'a>> {
    self_and_ancestors(ctx, id)
        .filter(|n| matches!(n.kind(), AstKind::ObjectExpression(_)))
        .find(|n| is_es5_component(ctx, n.id()))
}

/// `utils.getParentStatelessComponent`: component node of the nearest enclosing function that
/// is a function component (crossing functions).
pub fn parent_stateless_component(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<NodeId> {
    self_and_ancestors(ctx, id)
        .filter(|n| is_function(&n.kind()))
        .find_map(|n| stateless_component(ctx, n))
}

/// `utils.getParentComponent`: ES6 class, ES5 object or function component.
pub fn parent_component(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<NodeId> {
    parent_es6_component(ctx, id)
        .or_else(|| parent_es5_component(ctx, id))
        .map(|n| n.id())
        .or_else(|| parent_stateless_component(ctx, id))
}

/// `minimatch`-style glob with `*`, `?` and `{a,b}` alternatives.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if let Some(open) = pattern.find('{')
        && let Some(close) = pattern[open..].find('}').map(|c| c + open)
    {
        let (pre, alts, post) = (
            &pattern[..open],
            &pattern[open + 1..close],
            &pattern[close + 1..],
        );
        return alts
            .split(',')
            .any(|alt| glob_match(&format!("{pre}{alt}{post}"), text));
    }
    fn rec(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => (0..=t.len()).any(|i| rec(&p[1..], &t[i..])),
            Some(b'?') => !t.is_empty() && rec(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && rec(&p[1..], &t[1..]),
        }
    }
    rec(pattern.as_bytes(), text.as_bytes())
}

/// Members of a class body or of an ES5 component object with a static key name.
pub fn class_member_name<'a>(el: &ClassElement<'a>) -> Option<&'a str> {
    match el.property_key()? {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Props (propTypes / defaultProps / typed props / usages)
// ---------------------------------------------------------------------------------------------

/// A detected component together with the pieces needed to reason about its props.
pub struct ComponentInfo<'a> {
    /// Component node (function, class or `memo` / `forwardRef` call).
    pub node: NodeId,
    /// Function implementing a function component.
    pub func: Option<NodeId>,
    /// Class of a class component.
    pub class: Option<&'a Class<'a>>,
    /// Symbol the component is bound to (`function Foo`, `const Foo = ...`, `class Foo`).
    pub symbol: Option<SymbolId>,
}

fn binding_symbol(p: &BindingPattern<'_>) -> Option<SymbolId> {
    match p {
        BindingPattern::BindingIdentifier(id) => id.symbol_id.get(),
        _ => None,
    }
}

/// Describes the component whose node is `node`, if it is one.
pub fn component_info<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> Option<ComponentInfo<'a>> {
    match node.kind() {
        AstKind::Class(c) if is_es6_component(c) => {
            let symbol = c.id.as_ref().and_then(|i| i.symbol_id.get()).or_else(|| {
                match estree_parent(ctx, node.id()).kind() {
                    AstKind::VariableDeclarator(v) => binding_symbol(&v.id),
                    _ => None,
                }
            });
            Some(ComponentInfo {
                node: node.id(),
                func: None,
                class: Some(c),
                symbol,
            })
        }
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
            let comp = stateless_component(ctx, node)?;
            let own = match node.kind() {
                AstKind::Function(f) if f.is_declaration() => {
                    f.id.as_ref().and_then(|i| i.symbol_id.get())
                }
                _ => None,
            };
            let symbol = own.or_else(|| match estree_parent(ctx, comp).kind() {
                AstKind::VariableDeclarator(v) => binding_symbol(&v.id),
                _ => None,
            });
            Some(ComponentInfo {
                node: comp,
                func: Some(node.id()),
                class: None,
                symbol,
            })
        }
        _ => None,
    }
}

/// A static member of a component: `static propTypes = {}`, `static get propTypes()`,
/// or `Foo.propTypes = {}`. `Some(None)` when it exists but is not an object literal.
pub fn component_static<'a>(
    ctx: &Ctx<'a, '_>,
    comp: &ComponentInfo<'a>,
    name: &str,
) -> Option<Option<&'a ObjectExpression<'a>>> {
    let as_object = |e: &'a Expression<'a>| match strip_ts(e) {
        Expression::ObjectExpression(o) => Some(&**o),
        _ => None,
    };
    if let Some(class) = comp.class {
        for el in &class.body.body {
            match el {
                ClassElement::PropertyDefinition(p)
                    if p.r#static && class_member_name(el) == Some(name) =>
                {
                    return Some(p.value.as_ref().and_then(as_object));
                }
                ClassElement::MethodDefinition(m)
                    if m.r#static
                        && m.kind == MethodDefinitionKind::Get
                        && class_member_name(el) == Some(name) =>
                {
                    let ret = m.value.body.as_ref().and_then(|b| {
                        b.statements.iter().find_map(|s| match s {
                            Statement::ReturnStatement(r) => r.argument.as_ref(),
                            _ => None,
                        })
                    });
                    return Some(ret.and_then(as_object));
                }
                _ => {}
            }
        }
    }
    let sym = comp.symbol?;
    for r in ctx.scoping().get_resolved_references(sym) {
        let member = ctx.parent(r.node_id());
        let AstKind::StaticMemberExpression(m) = member.kind() else {
            continue;
        };
        if m.property.name != name {
            continue;
        }
        if let AstKind::AssignmentExpression(a) = ctx.parent_kind(member.id())
            && a.left.span() == m.span
        {
            return Some(as_object(&a.right));
        }
    }
    None
}

/// A declared prop.
pub struct DeclaredProp<'a> {
    pub name: &'a str,
    /// Location upstream reports on (the key).
    pub key_span: Span,
    /// The whole property / signature.
    pub span: Span,
    pub required: bool,
}

/// Props declared by a propTypes object literal; `None` if it cannot be fully resolved.
pub fn prop_types_keys<'a>(o: &'a ObjectExpression<'a>) -> Option<Vec<DeclaredProp<'a>>> {
    let mut out = Vec::new();
    for p in &o.properties {
        let ObjectPropertyKind::ObjectProperty(p) = p else {
            return None;
        };
        if p.computed {
            return None;
        }
        let name = property_key_name(&p.key)?;
        let required = matches!(strip_parens(&p.value), Expression::StaticMemberExpression(m) if m.property.name == "isRequired");
        out.push(DeclaredProp {
            name,
            key_span: p.key.span(),
            span: p.span,
            required,
        });
    }
    Some(out)
}

/// Members of a local, non-generic, heritage-free props type.
pub fn ts_props<'a>(
    ctx: &Ctx<'a, '_>,
    t: &'a TSType<'a>,
    depth: u8,
) -> Option<Vec<DeclaredProp<'a>>> {
    if depth > 3 {
        return None;
    }
    let members: &'a [TSSignature<'a>] = match t {
        TSType::TSTypeLiteral(l) => &l.members,
        TSType::TSParenthesizedType(p) => return ts_props(ctx, &p.type_annotation, depth + 1),
        TSType::TSTypeReference(r) => {
            if r.type_arguments.is_some() {
                return None;
            }
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return None;
            };
            let sym = ctx.scoping().get_reference(id.reference_id()).symbol_id()?;
            if ctx.scoping().symbol_declarations(sym).count() != 1 {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::TSInterfaceDeclaration(i)
                    if i.extends.is_empty() && i.type_parameters.is_none() =>
                {
                    &i.body.body
                }
                AstKind::TSTypeAliasDeclaration(a) if a.type_parameters.is_none() => {
                    return ts_props(ctx, &a.type_annotation, depth + 1);
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let mut out = Vec::new();
    for m in members {
        match m {
            TSSignature::TSPropertySignature(p) if !p.computed => {
                out.push(DeclaredProp {
                    name: property_key_name(&p.key)?,
                    key_span: p.key.span(),
                    span: p.span,
                    required: !p.optional,
                });
            }
            TSSignature::TSMethodSignature(p) if !p.computed => {
                out.push(DeclaredProp {
                    name: property_key_name(&p.key)?,
                    key_span: p.key.span(),
                    span: p.span,
                    required: !p.optional,
                });
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Props type of a component from TypeScript annotations: the first parameter's annotation,
/// `const Foo: FC<Props>` or `class Foo extends Component<Props>`.
pub fn ts_props_type<'a>(ctx: &Ctx<'a, '_>, comp: &ComponentInfo<'a>) -> Option<&'a TSType<'a>> {
    if let Some(class) = comp.class {
        let args = class.heritage.as_ref()?.type_arguments.as_ref()?;
        return args.params.first();
    }
    let func = ctx.node(comp.func?);
    let params = match func.kind() {
        AstKind::Function(f) => &f.params,
        AstKind::ArrowFunctionExpression(a) => &a.params,
        _ => return None,
    };
    if let Some(ann) = params
        .items
        .first()
        .and_then(|p| p.type_annotation.as_ref())
    {
        return Some(&ann.type_annotation);
    }
    if let AstKind::VariableDeclarator(v) = estree_parent(ctx, comp.node).kind()
        && let Some(ann) = &v.type_annotation
        && let TSType::TSTypeReference(r) = &ann.type_annotation
    {
        let fc = match &r.type_name {
            TSTypeName::IdentifierReference(id) => {
                matches!(id.name.as_str(), "FC" | "FunctionComponent")
            }
            TSTypeName::QualifiedName(q) => {
                matches!(&q.left, TSTypeName::IdentifierReference(l) if l.name == "React")
                    && matches!(q.right.name.as_str(), "FC" | "FunctionComponent")
            }
            _ => false,
        };
        if fc {
            return r.type_arguments.as_ref()?.params.first();
        }
    }
    None
}

/// A prop usage: the chain of names (`props.a.b` → `["a", "b"]`) and the reported node.
pub struct PropUsage {
    pub names: Vec<String>,
    pub span: Span,
}

/// Prop usages of a component; `escaped` is set when props are used in ways that can't be
/// tracked (passed around, spread, rest elements, computed keys).
#[derive(Default)]
pub struct PropUsages {
    pub list: Vec<PropUsage>,
    pub escaped: bool,
}

const OBJECT_PROTOTYPE: &[&str] = &[
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

impl PropUsages {
    fn destructure(&mut self, p: &ObjectPattern<'_>, parents: &[String]) {
        if p.rest.is_some() {
            self.escaped = true;
        }
        for bp in &p.properties {
            if bp.computed {
                self.escaped = true;
                break;
            }
            let Some(name) = property_key_name(&bp.key) else {
                break;
            };
            let mut names = parents.to_vec();
            names.push(name.to_string());
            self.list.push(PropUsage {
                names: names.clone(),
                span: bp.span,
            });
            let inner = match &bp.value {
                BindingPattern::AssignmentPattern(a) => &a.left,
                v => v,
            };
            if let BindingPattern::ObjectPattern(o) = inner {
                self.destructure(o, &names);
            }
        }
    }

    /// Member chain starting at the node `start` (whose object is the props object).
    fn member_chain(&mut self, ctx: &Ctx<'_, '_>, start: NodeId, parents: &[String]) {
        let mut names = parents.to_vec();
        let mut cur = ctx.node(start);
        loop {
            let (name, prop_span, mspan) = match cur.kind() {
                AstKind::StaticMemberExpression(m) => {
                    (m.property.name.to_string(), m.property.span, m.span)
                }
                AstKind::ComputedMemberExpression(m) => match &m.expression {
                    Expression::StringLiteral(s) => (s.value.to_string(), s.span, m.span),
                    _ => return,
                },
                _ => return,
            };
            if names.is_empty()
                && matches!(ctx.parent_kind(cur.id()), AstKind::AssignmentExpression(a) if a.left.span() == mspan)
            {
                return;
            }
            names.push(name.clone());
            if !OBJECT_PROTOTYPE.contains(&name.as_str()) {
                self.list.push(PropUsage {
                    names: names.clone(),
                    span: prop_span,
                });
            }
            let parent = ctx.parent(cur.id());
            match parent.kind() {
                AstKind::StaticMemberExpression(m) if m.object.span() == mspan => cur = parent,
                AstKind::ComputedMemberExpression(m) if m.object.span() == mspan => cur = parent,
                AstKind::VariableDeclarator(v) => {
                    if let BindingPattern::ObjectPattern(o) = &v.id {
                        self.destructure(o, &names);
                    }
                    return;
                }
                _ => return,
            }
        }
    }

    /// Usages through references of `sym` (a variable holding the props object).
    fn track_symbol(&mut self, ctx: &Ctx<'_, '_>, sym: SymbolId, comp: NodeId) {
        for r in ctx.scoping().get_resolved_references(sym) {
            let id = r.node_id();
            if parent_component(ctx, id) != Some(comp) {
                self.escaped = true;
                continue;
            }
            let parent = ctx.parent(id);
            let my = ctx.node(id).kind().span();
            match parent.kind() {
                AstKind::StaticMemberExpression(m) if m.object.span() == my => {
                    self.member_chain(ctx, parent.id(), &[])
                }
                AstKind::ComputedMemberExpression(m) if m.object.span() == my => {
                    if matches!(m.expression, Expression::StringLiteral(_)) {
                        self.member_chain(ctx, parent.id(), &[]);
                    } else {
                        self.escaped = true;
                    }
                }
                AstKind::VariableDeclarator(v)
                    if v.init.as_ref().is_some_and(|i| strip_ts(i).span() == my) =>
                {
                    match &v.id {
                        BindingPattern::ObjectPattern(o) => self.destructure(o, &[]),
                        _ => self.escaped = true,
                    }
                }
                _ => self.escaped = true,
            }
        }
    }
}

/// Collects the prop usages of a component (`props.x`, destructured parameters and variables,
/// `this.props.x`).
pub fn prop_usages(ctx: &Ctx<'_, '_>, comp: &ComponentInfo<'_>) -> PropUsages {
    let mut u = PropUsages::default();
    if let Some(func) = comp.func {
        let params = match ctx.node(func).kind() {
            AstKind::Function(f) => &f.params,
            AstKind::ArrowFunctionExpression(a) => &a.params,
            _ => return u,
        };
        let Some(first) = params.items.first() else {
            return u;
        };
        let pattern = match &first.pattern {
            BindingPattern::AssignmentPattern(a) => &a.left,
            p => p,
        };
        match pattern {
            BindingPattern::ObjectPattern(o) => u.destructure(o, &[]),
            BindingPattern::BindingIdentifier(id) if id.name == "props" => {
                if let Some(sym) = id.symbol_id.get() {
                    u.track_symbol(ctx, sym, comp.node);
                }
            }
            _ => u.escaped = true,
        }
        // JSX spreads of non-literals may forward any prop.
        let span = ctx.node(func).kind().span();
        scan_spreads(ctx, func, span, &mut u);
        return u;
    }
    let Some(class) = comp.class else { return u };
    let nodes = ctx.nodes();
    let mut i = comp.node.index() + 1;
    while i < nodes.len() {
        let n = nodes.get_node(NodeId::from_usize(i));
        i += 1;
        let sp = n.kind().span();
        if sp.start >= class.span.end {
            break;
        }
        match n.kind() {
            AstKind::StaticMemberExpression(m)
                if m.property.name == "props"
                    && matches!(strip_ts(&m.object), Expression::ThisExpression(_)) =>
            {
                if parent_es6_component(ctx, n.id()).map(|c| c.id()) != Some(comp.node) {
                    continue;
                }
                let parent = ctx.parent(n.id());
                match parent.kind() {
                    AstKind::StaticMemberExpression(pm) if pm.object.span() == sp => {
                        u.member_chain(ctx, parent.id(), &[])
                    }
                    AstKind::ComputedMemberExpression(pm) if pm.object.span() == sp => {
                        if matches!(pm.expression, Expression::StringLiteral(_)) {
                            u.member_chain(ctx, parent.id(), &[]);
                        } else {
                            u.escaped = true;
                        }
                    }
                    AstKind::VariableDeclarator(v) => match &v.id {
                        BindingPattern::ObjectPattern(o) => u.destructure(o, &[]),
                        _ => u.escaped = true,
                    },
                    _ => u.escaped = true,
                }
            }
            AstKind::JSXSpreadAttribute(s)
                if !matches!(strip_parens(&s.argument), Expression::ObjectExpression(_)) =>
            {
                u.escaped = true;
            }
            AstKind::FormalParameter(p) => {
                // `nextProps` / `prevProps` / `props` parameters of lifecycle methods.
                if let BindingPattern::BindingIdentifier(id) = &p.pattern
                    && matches!(id.name.as_str(), "props" | "nextProps" | "prevProps")
                {
                    u.escaped = true;
                }
            }
            _ => {}
        }
    }
    u
}

fn scan_spreads(ctx: &Ctx<'_, '_>, func: NodeId, span: Span, u: &mut PropUsages) {
    let nodes = ctx.nodes();
    let mut i = func.index() + 1;
    while i < nodes.len() {
        let n = nodes.get_node(NodeId::from_usize(i));
        i += 1;
        if n.kind().span().start >= span.end {
            break;
        }
        if let AstKind::JSXSpreadAttribute(s) = n.kind()
            && !matches!(strip_parens(&s.argument), Expression::ObjectExpression(_))
        {
            u.escaped = true;
            return;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------------------------

/// Module name of `require('x')`.
pub fn require_module<'a>(e: &Expression<'a>) -> Option<&'a str> {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return None;
    };
    let Expression::Identifier(id) = &c.callee else {
        return None;
    };
    if id.name != "require" || c.arguments.len() != 1 {
        return None;
    }
    match c.arguments[0].as_expression()? {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Fully qualified name of an expression in terms of imported modules, Sonar's
/// `getFullyQualifiedName`: `import { useState } from 'react'; useState(...)` → `react.useState`,
/// `import _ from 'lodash'; _.debounce` → `lodash.debounce`. `None` when not resolvable.
pub fn fqn(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<String> {
    let mut quals: Vec<String> = Vec::new();
    fqn_raw(ctx, e, &mut quals, 0).map(|s| s.strip_prefix("node:").map(str::to_string).unwrap_or(s))
}

fn reduce_member<'b, 'a>(
    mut e: &'b Expression<'a>,
    quals: &mut Vec<String>,
    stop_at_call: bool,
) -> &'b Expression<'a> {
    loop {
        e = match e {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::StaticMemberExpression(m) => {
                quals.push(m.property.name.to_string());
                &m.object
            }
            Expression::ComputedMemberExpression(m) => match &m.expression {
                Expression::StringLiteral(s) => {
                    quals.push(s.value.to_string());
                    &m.object
                }
                _ => return e,
            },
            Expression::CallExpression(c) if !stop_at_call && require_module(e).is_none() => {
                &c.callee
            }
            Expression::TaggedTemplateExpression(t) if !stop_at_call => &t.tag,
            Expression::NewExpression(n) if !stop_at_call => &n.callee,
            Expression::ChainExpression(c) => match &c.expression {
                ChainElement::CallExpression(call) if !stop_at_call => &call.callee,
                ChainElement::CallExpression(_) => return e,
                ChainElement::TSNonNullExpression(x) => &x.expression,
                m => match m.as_member_expression() {
                    Some(MemberExpression::StaticMemberExpression(m)) => {
                        quals.push(m.property.name.to_string());
                        &m.object
                    }
                    _ => return e,
                },
            },
            Expression::TSNonNullExpression(x) => &x.expression,
            _ => return e,
        };
    }
}

fn join(module: &str, quals: &[String]) -> String {
    let mut s = module.split('/').collect::<Vec<_>>().join(".");
    for q in quals.iter().rev() {
        s.push('.');
        s.push_str(q);
    }
    s
}

fn fqn_raw(
    ctx: &Ctx<'_, '_>,
    e: &Expression<'_>,
    quals: &mut Vec<String>,
    depth: u8,
) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let base = reduce_member(e, quals, false);
    if let Some(m) = require_module(base) {
        return Some(join(m, quals));
    }
    let Expression::Identifier(id) = base else {
        return None;
    };
    let sym = ctx.symbol_of(id)?;
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            if s.import_kind.is_type() {
                return None;
            }
            let imported = s.imported.name();
            if imported != "default" {
                quals.push(imported.to_string());
            }
            import_source(ctx, decl.id()).map(|src| join(src, quals))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            import_source(ctx, decl.id()).map(|src| join(src, quals))
        }
        AstKind::VariableDeclarator(v) => {
            let init = v.init.as_ref()?;
            if ctx
                .scoping()
                .get_resolved_references(sym)
                .any(|r| r.is_write())
            {
                return None;
            }
            match &v.id {
                BindingPattern::BindingIdentifier(_) => {}
                BindingPattern::ObjectPattern(p) => {
                    let prop = p.properties.iter().find(|bp| match &bp.value {
                        BindingPattern::BindingIdentifier(b) => b.symbol_id.get() == Some(sym),
                        BindingPattern::AssignmentPattern(a) => {
                            matches!(&a.left, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                        }
                        _ => false,
                    })?;
                    quals.push(property_key_name(&prop.key)?.to_string());
                }
                _ => return None,
            }
            let target = reduce_member(init, quals, true);
            if let Some(m) = require_module(target) {
                return Some(join(m, quals));
            }
            fqn_raw(ctx, target, quals, depth + 1)
        }
        AstKind::TSImportEqualsDeclaration(d) => match &d.module_reference {
            TSModuleReference::ExternalModuleReference(r) => {
                Some(join(r.expression.value.as_str(), quals))
            }
            _ => None,
        },
        _ => None,
    }
}

fn import_source<'a>(ctx: &Ctx<'a, '_>, spec: NodeId) -> Option<&'a str> {
    ancestors(ctx, spec).find_map(|n| match n.kind() {
        AstKind::ImportDeclaration(d) => {
            if d.import_kind.is_type() {
                None
            } else {
                Some(d.source.value.as_str())
            }
        }
        _ => None,
    })
}

/// True if `e` resolves (see [`fqn`]) to `react.<name>`.
pub fn is_react_api(ctx: &Ctx<'_, '_>, e: &Expression<'_>, name: &str) -> bool {
    fqn(ctx, e).is_some_and(|f| f.strip_prefix("react.") == Some(name))
}

/// True if the file looks like React code: it contains JSX or imports / requires a module named
/// `react*` (react, react-dom, react-native, ...) or `preact*` at top level.
pub fn file_uses_react(ctx: &Ctx<'_, '_>) -> bool {
    let reactish = |m: &str| {
        m.starts_with("react") || m.starts_with("preact") || m.starts_with("@testing-library/react")
    };
    if ctx.nodes().contains(AstType::JSXElement) || ctx.nodes().contains(AstType::JSXFragment) {
        return true;
    }
    ctx.nodes().program().body.iter().any(|s| match s {
        Statement::ImportDeclaration(d) => reactish(&d.source.value),
        Statement::VariableDeclaration(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .and_then(require_module)
                .is_some_and(reactish)
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_first_letter_capitalized("Foo"));
        assert!(is_first_letter_capitalized("_Foo"));
        assert!(!is_first_letter_capitalized("_foo"));
        assert!(!is_first_letter_capitalized("foo"));
        assert!(is_hook_name("useState"));
        assert!(is_hook_name("use"));
        assert!(is_hook_name("use1"));
        assert!(!is_hook_name("user"));
        assert!(!is_hook_name("useless"));
    }
}
