//! Deprecated React APIs should not be used
//!
//! `react/no-deprecated`, evaluated against the latest React version (as Sonar does when the
//! project's React version is unknown).
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// (old name, version, replacement)
const DEPRECATED: &[(&str, &str, Option<&str>)] = &[
    ("React.renderComponent", "0.12.0", Some("React.render")),
    (
        "React.renderComponentToString",
        "0.12.0",
        Some("React.renderToString"),
    ),
    (
        "React.renderComponentToStaticMarkup",
        "0.12.0",
        Some("React.renderToStaticMarkup"),
    ),
    (
        "React.isValidComponent",
        "0.12.0",
        Some("React.isValidElement"),
    ),
    (
        "React.PropTypes.component",
        "0.12.0",
        Some("React.PropTypes.element"),
    ),
    (
        "React.PropTypes.renderable",
        "0.12.0",
        Some("React.PropTypes.node"),
    ),
    ("React.isValidClass", "0.12.0", None),
    (
        "this.transferPropsTo",
        "0.12.0",
        Some("spread operator ({...})"),
    ),
    (
        "React.addons.classSet",
        "0.13.0",
        Some("the npm module classnames"),
    ),
    (
        "React.addons.cloneWithProps",
        "0.13.0",
        Some("React.cloneElement"),
    ),
    ("React.render", "0.14.0", Some("ReactDOM.render")),
    (
        "React.unmountComponentAtNode",
        "0.14.0",
        Some("ReactDOM.unmountComponentAtNode"),
    ),
    ("React.findDOMNode", "0.14.0", Some("ReactDOM.findDOMNode")),
    (
        "React.renderToString",
        "0.14.0",
        Some("ReactDOMServer.renderToString"),
    ),
    (
        "React.renderToStaticMarkup",
        "0.14.0",
        Some("ReactDOMServer.renderToStaticMarkup"),
    ),
    ("React.addons.LinkedStateMixin", "15.0.0", None),
    (
        "ReactPerf.printDOM",
        "15.0.0",
        Some("ReactPerf.printOperations"),
    ),
    ("Perf.printDOM", "15.0.0", Some("Perf.printOperations")),
    (
        "ReactPerf.getMeasurementsSummaryMap",
        "15.0.0",
        Some("ReactPerf.getWasted"),
    ),
    (
        "Perf.getMeasurementsSummaryMap",
        "15.0.0",
        Some("Perf.getWasted"),
    ),
    (
        "React.createClass",
        "15.5.0",
        Some("the npm module create-react-class"),
    ),
    (
        "React.addons.TestUtils",
        "15.5.0",
        Some("ReactDOM.TestUtils"),
    ),
    (
        "React.PropTypes",
        "15.5.0",
        Some("the npm module prop-types"),
    ),
    (
        "React.DOM",
        "15.6.0",
        Some("the npm module react-dom-factories"),
    ),
    (
        "componentWillMount",
        "16.9.0",
        Some("UNSAFE_componentWillMount"),
    ),
    (
        "componentWillReceiveProps",
        "16.9.0",
        Some("UNSAFE_componentWillReceiveProps"),
    ),
    (
        "componentWillUpdate",
        "16.9.0",
        Some("UNSAFE_componentWillUpdate"),
    ),
    ("ReactDOM.render", "18.0.0", Some("createRoot")),
    ("ReactDOM.hydrate", "18.0.0", Some("hydrateRoot")),
    (
        "ReactDOM.unmountComponentAtNode",
        "18.0.0",
        Some("root.unmount"),
    ),
    (
        "ReactDOMServer.renderToNodeStream",
        "18.0.0",
        Some("renderToPipeableStream"),
    ),
];

fn module_name(source: &str) -> Option<&'static str> {
    match source {
        "react" => Some("React"),
        "react-addons-perf" => Some("ReactPerf"),
        "react-dom" => Some("ReactDOM"),
        "react-dom/server" => Some("ReactDOMServer"),
        _ => None,
    }
}

fn check(ctx: &Ctx<'_, '_>, name: &str, span: Span) {
    if let Some((old, version, new)) = DEPRECATED.iter().find(|(o, _, _)| *o == name) {
        let new = new
            .map(|n| format!(", use {n} instead"))
            .unwrap_or_default();
        report_react(
            ctx,
            span,
            format!("{old} is deprecated since React {version}{new}"),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
            AstType::ImportDeclaration,
            AstType::VariableDeclarator,
            AstType::Class,
            AstType::ObjectExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StaticMemberExpression(m) => check(ctx, ctx.text(m.span), m.span),
            AstKind::ComputedMemberExpression(m) => check(ctx, ctx.text(m.span), m.span),
            AstKind::ImportDeclaration(d) => {
                let Some(module) = module_name(&d.source.value) else {
                    return;
                };
                for s in d.specifiers.iter().flatten() {
                    if let ImportDeclarationSpecifier::ImportSpecifier(s) = s {
                        check(ctx, &format!("{module}.{}", s.imported.name()), s.span);
                    }
                }
            }
            AstKind::VariableDeclarator(v) => {
                let BindingPattern::ObjectPattern(p) = &v.id else {
                    return;
                };
                let Some(init) = &v.init else { return };
                let module = match require_module(init) {
                    Some(m) => match module_name(m) {
                        Some(n) => Some(n),
                        None => return,
                    },
                    None => match init {
                        Expression::Identifier(id) => {
                            ["React", "ReactPerf", "Perf", "ReactDOM", "ReactDOMServer"]
                                .into_iter()
                                .find(|n| *n == id.name)
                        }
                        _ => None,
                    },
                };
                let Some(module) = module else { return };
                for prop in &p.properties {
                    if let PropertyKey::StaticIdentifier(k) = &prop.key {
                        check(ctx, &format!("{module}.{}", k.name), prop.span);
                    }
                }
            }
            AstKind::Class(c) if is_es6_component(c) => {
                for el in &c.body.body {
                    if let (Some(name), Some(key)) = (class_member_name(el), el.property_key()) {
                        check(ctx, name, key.span());
                    }
                }
            }
            AstKind::ObjectExpression(o) if is_es5_component(ctx, node.id()) => {
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p
                        && let Some(name) = property_key_name(&p.key)
                    {
                        check(ctx, name, p.key.span());
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
    fn deprecated() {
        check(
            "S6957",
            "a.jsx",
            r#"
import React, { PropTypes, Component } from 'react'; // Noncompliant {{React.PropTypes is deprecated since React 15.5.0, use the npm module prop-types instead}}
import ReactDOM from 'react-dom';
React.render(<MyComponent />, root); // Noncompliant {{React.render is deprecated since React 0.14.0, use ReactDOM.render instead}}
React.createClass({}); // Noncompliant {{React.createClass is deprecated since React 15.5.0, use the npm module create-react-class instead}}
React.DOM.div(); // Noncompliant {{React.DOM is deprecated since React 15.6.0, use the npm module react-dom-factories instead}}
React.isValidClass(x); // Noncompliant {{React.isValidClass is deprecated since React 0.12.0}}
class ApiCall extends Component {
  componentWillMount() {} // Noncompliant {{componentWillMount is deprecated since React 16.9.0, use UNSAFE_componentWillMount instead}}
  componentDidMount() {}
}
ReactDOM.render(<div></div>, container); // Noncompliant {{ReactDOM.render is deprecated since React 18.0.0, use createRoot instead}}
ReactDOMServer.renderToNodeStream(element); // Noncompliant
const { render } = require('react-dom'); // Noncompliant {{ReactDOM.render is deprecated since React 18.0.0, use createRoot instead}}
const { createElement } = React;
React.createElement('div');
"#,
        );
    }
}
