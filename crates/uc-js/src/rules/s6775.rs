//! All "defaultProps" should have non-required PropTypes
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::Class,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(comp) = component_info(ctx, node) else {
            return;
        };
        let Some(Some(defaults)) = component_static(ctx, &comp, "defaultProps") else {
            return;
        };
        let Some(defaults) = prop_types_keys(defaults) else {
            return;
        };
        let declared = match component_static(ctx, &comp, "propTypes") {
            Some(None) => return,
            Some(Some(o)) => match prop_types_keys(o) {
                Some(k) => k,
                None => return,
            },
            // Context-typed class components (third type argument) are not inferred upstream.
            None if comp.class.is_some_and(|c| {
                c.heritage
                    .as_ref()
                    .and_then(|h| h.type_arguments.as_ref())
                    .is_some_and(|a| a.params.len() > 2)
            }) =>
            {
                return;
            }
            None => match ts_props_type(ctx, &comp).and_then(|t| ts_props(ctx, t, 0)) {
                Some(k) => k,
                None => return,
            },
        };
        if declared.is_empty() {
            return;
        }
        for d in defaults {
            match declared.iter().find(|p| p.name == d.name) {
                Some(p) if !p.required => {}
                Some(_) => ctx.report(
                    d.span,
                    format!(
                        "defaultProp \"{}\" defined for isRequired propType.",
                        d.name
                    ),
                ),
                None => ctx.report(
                    d.span,
                    format!(
                        "defaultProp \"{}\" has no corresponding propTypes declaration.",
                        d.name
                    ),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn default_props() {
        check(
            "S6775",
            "a.jsx",
            r#"
import React from "react";
function Hello(props) { return <div>{props.name}</div>; }
Hello.propTypes = {
  name: PropTypes.string.isRequired,
  title: PropTypes.string,
};
Hello.defaultProps = {
  name: 'x', // Noncompliant {{defaultProp "name" defined for isRequired propType.}}
  title: 'y',
  other: 1, // Noncompliant {{defaultProp "other" has no corresponding propTypes declaration.}}
};
class K extends React.Component {
  static propTypes = { a: PropTypes.string };
  static defaultProps = { a: '', b: '' }; // Noncompliant
  render() { return null; }
}
function NoTypes(props) { return <div />; }
NoTypes.defaultProps = { a: 1 };
"#,
        );
    }

    #[test]
    fn default_props_ts() {
        check(
            "S6775",
            "a.tsx",
            r#"
import React from "react";
interface Props { a: string; b?: string; }
function Foo(props: Props) { return <div>{props.a}</div>; }
Foo.defaultProps = {
  a: 'x', // Noncompliant
  b: 'y',
};
"#,
        );
    }
}
