//! React components should not be nested
//!
//! Follows `react/no-unstable-nested-components` with Sonar's message, location and react-intl
//! exemption.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    allow_as_props: bool,
    pattern: String,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let pattern = p.str("propNamePattern", "{render*,*Enhancer,*Renderer}");
        Self {
            allow_as_props: p.bool("allowAsProps", false),
            pattern: pattern.trim_matches('\'').to_string(),
        }
    }
}

const MSG: &str =
    "Move this component definition out of the parent component and pass data as props.";
const INTL: &[&str] = &[
    "FormattedMessage",
    "FormattedHTMLMessage",
    "FormattedPlural",
    "$t",
];

fn returns_jsx_node(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    is_function(&node.kind()) && returns_jsx(ctx, &node.kind())
}

fn property_key<'a>(kind: &AstKind<'a>) -> Option<&'a str> {
    match kind {
        AstKind::ObjectProperty(p) => key_name(&p.key),
        _ => None,
    }
}

fn key_name<'a>(k: &PropertyKey<'a>) -> Option<&'a str> {
    match k {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// Closest ancestor (from the parent) matching `f`.
fn closest<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
    mut f: impl FnMut(&AstNode<'a>) -> bool,
) -> Option<&'s AstNode<'a>> {
    ancestors(ctx, id)
        .take_while(|n| !matches!(n.kind(), AstKind::Program(_)))
        .find(|n| f(n))
}

fn is_attr_with_container(n: &AstNode<'_>) -> bool {
    matches!(n.kind(), AstKind::JSXAttribute(a) if matches!(a.value, Some(JSXAttributeValue::ExpressionContainer(_))))
}

impl Rule {
    fn is_declared_inside_props(&self, ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
        if matches!(
            estree_parent(ctx, node.id()).kind(),
            AstKind::ObjectProperty(_)
        ) {
            return returns_jsx_node(ctx, node);
        }
        if closest(ctx, node.id(), is_attr_with_container).is_none() {
            // isComponentInsideCreateElementsProp
            if !is_component_node(ctx, node) {
                return false;
            }
            let call = closest(
                ctx,
                node.id(),
                |n| matches!(n.kind(), AstKind::CallExpression(c) if is_create_element(ctx, c)),
            );
            let obj = closest(ctx, node.id(), |n| {
                matches!(n.kind(), AstKind::ObjectExpression(_))
            });
            return match (call.map(|c| c.kind()), obj) {
                (Some(AstKind::CallExpression(c)), Some(o)) => c
                    .arguments
                    .get(1)
                    .is_some_and(|a| a.span() == o.kind().span()),
                _ => false,
            };
        }
        returns_jsx_node(ctx, node)
    }

    fn is_in_render_prop(&self, ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
        let parent = estree_parent(ctx, node.id());
        if property_key(&parent.kind()).is_some_and(|k| glob_match(&self.pattern, k)) {
            return true;
        }
        if matches!(parent.kind(), AstKind::JSXExpressionContainer(_))
            && matches!(ctx.parent_kind(parent.id()), AstKind::JSXElement(_))
        {
            return true;
        }
        let Some(container) = closest(ctx, node.id(), |n| {
            matches!(n.kind(), AstKind::JSXExpressionContainer(_))
        }) else {
            return false;
        };
        if let AstKind::JSXAttribute(a) = ctx.parent_kind(container.id())
            && let Some(name) = attr_name(a)
        {
            return glob_match(&self.pattern, name) || name == "children";
        }
        false
    }
}

fn is_map_call(kind: &AstKind<'_>) -> bool {
    matches!(kind, AstKind::CallExpression(c) if member_name(&c.callee) == Some("map"))
}

fn member_name<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match e {
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        _ => None,
    }
}

fn is_return_of_hook(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    if !matches!(
        estree_parent(ctx, node.id()).kind(),
        AstKind::ReturnStatement(_)
    ) {
        return false;
    }
    let Some(call) = closest(ctx, node.id(), |n| {
        matches!(n.kind(), AstKind::CallExpression(_))
    }) else {
        return false;
    };
    let AstKind::CallExpression(c) = call.kind() else {
        return false;
    };
    matches!(&c.callee, Expression::Identifier(id) if id.name.len() > 3 && id.name.starts_with("use")
        && (id.name.as_bytes()[3].is_ascii_uppercase() || id.name.as_bytes()[3].is_ascii_digit()))
}

/// Sonar: functions used as react-intl rich text formatting values are render callbacks.
fn is_react_intl_value(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    if !is_function(&node.kind()) {
        return false;
    }
    let prop = ctx.parent(node.id());
    if !matches!(prop.kind(), AstKind::ObjectProperty(_)) {
        return false;
    }
    let obj = ctx.parent(prop.id());
    let AstKind::ObjectExpression(o) = obj.kind() else {
        return false;
    };
    let container = estree_parent(ctx, obj.id());
    match container.kind() {
        AstKind::JSXExpressionContainer(_) => {
            let AstKind::JSXAttribute(a) = ctx.parent_kind(container.id()) else {
                return false;
            };
            let AstKind::JSXOpeningElement(el) = ctx.parent_kind(ctx.parent(container.id()).id())
            else {
                return false;
            };
            attr_name(a) == Some("values")
                && matches!(&el.name, JSXElementName::IdentifierReference(id) if INTL.contains(&id.name.as_str()))
        }
        AstKind::CallExpression(c) => {
            c.arguments.get(1).is_some_and(|a| a.span() == o.span)
                && member_name(&c.callee) == Some("formatMessage")
        }
        _ => false,
    }
}

fn component_name<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> Option<&'a str> {
    let own = match node.kind() {
        AstKind::Function(f) => f.id.as_ref().map(|i| i.name.as_str()),
        AstKind::Class(c) => c.id.as_ref().map(|i| i.name.as_str()),
        _ => None,
    };
    own.or_else(|| match node.kind() {
        AstKind::ArrowFunctionExpression(_) => match estree_parent(ctx, node.id()).kind() {
            AstKind::VariableDeclarator(v) => match &v.id {
                BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::Class,
            AstType::CallExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Function(f) if f.body.is_none() => return,
            AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::CallExpression(_) => {}
            AstKind::Class(c) if c.is_declaration() => {}
            _ => return,
        }
        let in_props = self.is_declared_inside_props(ctx, node);
        if !is_component_node(ctx, node) && !in_props && !function_component_inside_class(ctx, node)
        {
            return;
        }
        let parent = estree_parent(ctx, node.id());
        if (in_props && (self.allow_as_props || self.is_in_render_prop(ctx, node)))
            || is_map_call(&node.kind())
            || is_map_call(&parent.kind())
            || is_return_of_hook(ctx, node)
            || property_key(&parent.kind()).is_some_and(|k| glob_match(&self.pattern, k))
            || inside_render_method(ctx, node)
            || returning_null_component(ctx, node)
            || is_react_intl_value(ctx, node)
        {
            return;
        }
        let Some(parent_component) = closest(ctx, node.id(), |n| is_component_node(ctx, n)) else {
            return;
        };
        if let Some(name) = component_name(ctx, parent_component)
            && name
                .chars()
                .next()
                .is_some_and(|c| c.to_lowercase().eq(std::iter::once(c)))
        {
            return;
        }
        let span = match node.kind() {
            AstKind::Class(c) => {
                c.id.as_ref()
                    .map(|i| i.span)
                    .or_else(|| find_token(ctx, c.span, Kind::Class))
                    .unwrap_or(c.span)
            }
            AstKind::CallExpression(c) => c.span,
            _ => function_report_span(ctx, node),
        };
        report_react(ctx, span, MSG);
    }
}

fn function_component_inside_class(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let Some(pc) = parent_component(ctx, node.id()) else {
        return false;
    };
    if !matches!(ctx.node(pc).kind(), AstKind::Class(c) if c.is_declaration()) {
        return false;
    }
    let Some(ps) = parent_stateless_component(ctx, node.id()) else {
        return false;
    };
    let ps = ctx.node(ps);
    is_function(&ps.kind()) && stateless_component(ctx, ps).is_some() && returns_jsx_node(ctx, node)
}

fn inside_render_method(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let Some(pc) = parent_component(ctx, node.id()) else {
        return false;
    };
    matches!(ctx.node(pc).kind(), AstKind::Class(c) if c.is_declaration())
        && matches!(ctx.parent_kind(node.id()), AstKind::MethodDefinition(m) if key_name(&m.key) == Some("render"))
}

fn returning_null_component(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    if !is_function(&node.kind()) {
        return false;
    }
    match stateless_component(ctx, node) {
        Some(c) => !returns_jsx_node(ctx, ctx.node(c)),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_components() {
        check(
            "S6478",
            "a.jsx",
            r#"
function Component1() {
    function NestedComponent1() { // Noncompliant {{Move this component definition out of the parent component and pass data as props.}}
        return <div />;
    }
    return <div><NestedComponent1 /></div>;
}

function Component2() {
    class NestedComponent2 extends React.Component { // Noncompliant
        render() {
            return <div />;
        }
    };
    return <div><NestedComponent2 /></div>;
}

function Component3() {
    return (
        <FormattedMessage
            values={{
                b: chunks => <b>{chunks}</b>,
            }}
        />
    );
}

function Component4() {
    const intl = useIntl();
    return <p>{intl.formatMessage({ id: 'greeting' }, { b: chunks => <b>{chunks}</b> })}</p>;
}

function Component6() {
    return (
        <SomeOtherComponent
            values={{
                Item: () => <div />, // Noncompliant
            }}
        />
    );
}

function Component7() {
    const renderers = {
        Bold: (chunks) => <b>{chunks}</b>, // Noncompliant
    };
    return <Something renderers={renderers} />;
}

function Fine() {
    const items = list.map(item => <li>{item}</li>);
    const renderRow = () => <tr />;
    const cb = useCallback(() => <div />, []);
    return (
        <Table
            renderFooter={() => <div />}
            footer={<div />}
        >
            {(value) => <div>{value}</div>}
        </Table>
    );
}

function Inline() {
    return <Table cell={() => <div />} />; // Noncompliant
}

function createThing() {
    function Inner() { return <div />; }
    return Inner;
}

const Outer = () => {
    const Inner = () => <div />; // Noncompliant
    return <Inner />;
};

class Klass extends React.Component {
    render() {
        const Row = () => <tr />; // Noncompliant
        return <Row />;
    }
}
"#,
        );
    }
}
