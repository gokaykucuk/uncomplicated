//! Prefer tag over ARIA role
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const COMPOSITE_CONTAINER_ROLES: &[&str] = &["table", "grid", "listbox"];
const COMPOSITE_CHILD_ROLES: &[&str] = &[
    "row",
    "rowgroup",
    "cell",
    "gridcell",
    "columnheader",
    "rowheader",
    "option",
];

enum Pos<'a> {
    Child(&'a JSXChild<'a>),
    Expr(&'a Expression<'a>),
    Stmt(&'a Statement<'a>),
}

fn is_mapping_call(call: &CallExpression<'_>) -> bool {
    matches!(strip_parens(&call.callee), Expression::StaticMemberExpression(m) if m.property.name == "map" || m.property.name == "flatMap")
}

/// SonarJS `someRenderedJsxDescendant`: JSX elements rendered under `el`, through conditionals,
/// arrays and `.map()` callbacks; `prune` stops the descent below an element.
fn some_rendered_descendant<'a>(
    el: &'a JSXElement<'a>,
    pred: &dyn Fn(&'a JSXElement<'a>) -> bool,
    prune: &dyn Fn(&'a JSXElement<'a>) -> bool,
) -> bool {
    let mut stack: Vec<Pos<'a>> = el.children.iter().map(Pos::Child).collect();
    while let Some(p) = stack.pop() {
        let element = match p {
            Pos::Child(JSXChild::Element(e)) => Some(&**e),
            Pos::Child(JSXChild::Fragment(f)) => {
                stack.extend(f.children.iter().map(Pos::Child));
                None
            }
            Pos::Child(JSXChild::ExpressionContainer(c)) => {
                if let Some(e) = c.expression.as_expression() {
                    stack.push(Pos::Expr(e));
                }
                None
            }
            Pos::Child(_) => None,
            Pos::Expr(e) => match strip_ts(e) {
                Expression::JSXElement(e) => Some(&**e),
                Expression::JSXFragment(f) => {
                    stack.extend(f.children.iter().map(Pos::Child));
                    None
                }
                Expression::ConditionalExpression(c) => {
                    stack.push(Pos::Expr(&c.consequent));
                    stack.push(Pos::Expr(&c.alternate));
                    None
                }
                Expression::LogicalExpression(l) => {
                    if l.operator != LogicalOperator::And {
                        stack.push(Pos::Expr(&l.left));
                    }
                    stack.push(Pos::Expr(&l.right));
                    None
                }
                Expression::ArrayExpression(a) => {
                    stack.extend(
                        a.elements
                            .iter()
                            .filter_map(|e| e.as_expression())
                            .map(Pos::Expr),
                    );
                    None
                }
                Expression::CallExpression(call) if is_mapping_call(call) => {
                    for arg in &call.arguments {
                        match arg.as_expression().map(strip_parens) {
                            Some(Expression::ArrowFunctionExpression(f)) => {
                                if let Some(e) = f.body.as_expression() {
                                    stack.push(Pos::Expr(e));
                                } else if let Some(body) = f.body.as_function_body() {
                                    stack.extend(body.statements.iter().map(Pos::Stmt));
                                }
                            }
                            Some(Expression::FunctionExpression(f)) => {
                                if let Some(body) = &f.body {
                                    stack.extend(body.statements.iter().map(Pos::Stmt));
                                }
                            }
                            _ => {}
                        }
                    }
                    None
                }
                _ => None,
            },
            Pos::Stmt(s) => {
                match s {
                    Statement::ReturnStatement(r) => {
                        if let Some(a) = &r.argument {
                            stack.push(Pos::Expr(a));
                        }
                    }
                    Statement::BlockStatement(b) => stack.extend(b.body.iter().map(Pos::Stmt)),
                    Statement::IfStatement(i) => {
                        stack.push(Pos::Stmt(&i.consequent));
                        if let Some(a) = &i.alternate {
                            stack.push(Pos::Stmt(a));
                        }
                    }
                    Statement::SwitchStatement(sw) => {
                        for case in &sw.cases {
                            stack.extend(case.consequent.iter().map(Pos::Stmt));
                        }
                    }
                    _ => {}
                }
                None
            }
        };
        if let Some(e) = element {
            if pred(e) {
                return true;
            }
            if !prune(e) {
                stack.extend(e.children.iter().map(Pos::Child));
            }
        }
    }
    false
}

fn role_of<'a>(e: &'a JSXElement<'a>) -> Option<String> {
    role_string(&e.opening_element)
}

fn has_ancestor_role<'a>(ctx: &Ctx<'a, '_>, element_id: NodeId, roles: &[&str]) -> bool {
    ctx.ancestors(element_id).any(|n| {
        matches!(n.kind(), AstKind::JSXElement(e) if role_of(e).is_some_and(|r| roles.contains(&r.as_str())))
    })
}

fn has_style_background_image<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    let Some(PropValue::Expr(e)) = get_prop(el, "style").map(|p| p.value) else {
        return false;
    };
    let Expression::ObjectExpression(o) = strip_parens(e) else {
        return false;
    };
    o.properties.iter().any(|p| {
        matches!(p, ObjectPropertyKind::ObjectProperty(p) if property_key_name(&p.key) == Some("backgroundImage"))
    })
}

fn has_svg_accessible_name<'a>(el: &'a JSXOpeningElement<'a>, element: &'a JSXElement<'a>) -> bool {
    has_accessible_name_attribute(el, "aria-labelledby")
        || has_accessible_name_attribute(el, "aria-label")
        || element.children.iter().any(|c| {
            let JSXChild::Element(t) = c else {
                return false;
            };
            matches!(&t.opening_element.name, JSXElementName::Identifier(id) if id.name == "title")
                && t.children.iter().any(|c| match c {
                    JSXChild::Text(t) => !t.value.trim().is_empty(),
                    JSXChild::ExpressionContainer(c) => {
                        match c.expression.as_expression().map(strip_parens) {
                            None | Some(Expression::NullLiteral(_)) => false,
                            Some(Expression::StringLiteral(s)) => !s.value.is_empty(),
                            Some(Expression::NumericLiteral(n)) => n.value != 0.0,
                            Some(Expression::BooleanLiteral(b)) => b.value,
                            Some(Expression::Identifier(id)) => id.name != "undefined",
                            Some(_) => true,
                        }
                    }
                    _ => false,
                })
        })
}

/// Valid ARIA patterns SonarJS does not report.
fn is_false_positive<'a>(
    ctx: &Ctx<'a, '_>,
    element_id: NodeId,
    element: &'a JSXElement<'a>,
) -> bool {
    let el = &element.opening_element;
    let Some(role) = role_string(el) else {
        return false;
    };
    let name = element_name(el).to_ascii_lowercase();
    let role = role.as_str();
    let has = |p: &str| get_prop(el, p).is_some();
    let has_children = !element.children.is_empty();
    if name == "svg" && (role == "presentation" || role == "img") {
        let hidden = get_prop(el, "aria-hidden").map(|p| literal_value(&p));
        if matches!(hidden, Some(Val::Bool(true))) {
            return true;
        }
    }
    if name == "svg" && role == "img" && has_svg_accessible_name(el, element) {
        return true;
    }
    let matched = match role {
        "status" => has("aria-live"),
        "slider" => has("aria-valuemin") && has("aria-valuemax") && has("aria-valuenow"),
        "radio" => has("aria-checked"),
        "combobox" => {
            has("aria-expanded")
                && has_any_prop(el, &["aria-controls", "aria-owns", "aria-haspopup"])
        }
        "separator" => has_children,
        "img" => {
            (name == "div" || name == "span") && (has_children || has_style_background_image(el))
        }
        _ => false,
    };
    if matched {
        return true;
    }
    if role == "group"
        && has_ancestor_role(ctx, element_id, &["listbox"])
        && some_rendered_descendant(
            element,
            &|d| role_of(d).as_deref() == Some("option"),
            &|d| role_of(d).as_deref() == Some("listbox"),
        )
    {
        return true;
    }
    if COMPOSITE_CONTAINER_ROLES.contains(&role) {
        return some_rendered_descendant(
            element,
            &|d| role_of(d).is_some_and(|r| COMPOSITE_CHILD_ROLES.contains(&r.as_str())),
            &|_| false,
        );
    }
    if COMPOSITE_CHILD_ROLES.contains(&role) {
        return has_ancestor_role(ctx, element_id, COMPOSITE_CONTAINER_ROLES);
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let Some(p) = get_prop(el, "role") else {
            return;
        };
        let Val::Str(value) = prop_value(&p) else {
            return;
        };
        if value.is_empty() || is_interpolated(&p) {
            return;
        }
        let role = value.rsplit(' ').next().unwrap_or(&value);
        let Some((tags, names)) = role_tags(role) else {
            return;
        };
        let JSXElementName::Identifier(id) = &el.name else {
            return;
        };
        let name = id.name.as_str();
        if names.contains(&name) || !is_html_tag(name) {
            return;
        }
        let element_node = ctx.parent(node.id());
        let AstKind::JSXElement(element) = element_node.kind() else {
            return;
        };
        if is_false_positive(ctx, element_node.id(), element) {
            return;
        }
        ctx.report(el.span, format!("Use {tags} instead of the \"{role}\" role to ensure accessibility across all devices."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_tag() {
        check(
            "S6819",
            "a.jsx",
            r##"
<div role="checkbox" />; // Noncompliant {{Use <input type="checkbox"> instead of the "checkbox" role to ensure accessibility across all devices.}}
<div role="heading" />; // Noncompliant {{Use <h1>, <h2>, <h3>, <h4>, <h5>, or <h6> instead of the "heading" role to ensure accessibility across all devices.}}
<span role="link" />; // Noncompliant {{Use <a href=...>, or <area href=...> instead of the "link" role to ensure accessibility across all devices.}}
<div role="presentation button" />; // Noncompliant
<div role="img" aria-label="picture" />; // Noncompliant
<span role="img" aria-label="icon" />; // Noncompliant
<svg role="img"><path /></svg>; // Noncompliant
<svg role="img" aria-label=""><path /></svg>; // Noncompliant
<div role="row"><div role="cell">Data</div></div>; // Noncompliant 2
<div role="table"><div>hello</div></div>; // Noncompliant
<div role="columnheader">Header</div>; // Noncompliant
<div role="combobox" aria-expanded={isOpen}><input /></div>; // Noncompliant
<div role="button" onClick={toggle}>Open</div>; // Noncompliant
<section role="region">content</section>;
<nav role="navigation">content</nav>;
<mat-card role="region">content</mat-card>;
<Foo role="button" />;
<div role={role} />;
<div role="Button" />;
<div role="foo" />;
<span role="img" aria-label="computer">💻</span>;
<div role="img" style={{ backgroundImage: `url(${src})` }} />;
<svg role="img" aria-label="Arrow Down"><path /></svg>;
<svg role="img"><title>Settings</title></svg>;
<svg role="img" aria-hidden="true" />;
<div role="status" aria-live="polite" />;
<div role="slider" aria-valuemin="0" aria-valuemax="9" aria-valuenow="3" />;
<div role="radio" aria-checked="false" />;
<div role="combobox" aria-expanded={isOpen} aria-controls="choices">Selected</div>;
<div role="separator">x</div>;
<div role="listbox"><div role="option">Item</div></div>;
<div role="listbox"><div role="group">{items.map(item => <div role="option">{item}</div>)}</div></div>;
<div role="grid">{rows.map(r => { return <div role="row">{r}</div>; })}</div>;
<div role="table">{cond && <div role="rowgroup" />}</div>;
"##,
        );
    }
}
