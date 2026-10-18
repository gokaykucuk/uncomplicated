//! React components should validate prop types
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
        let declared: Vec<&str> = match component_static(ctx, &comp, "propTypes") {
            None => Vec::new(),
            Some(None) => return,
            Some(Some(o)) => match prop_types_keys(o) {
                Some(keys) => keys.into_iter().map(|k| k.name).collect(),
                None => return,
            },
        };
        let usages = prop_usages(ctx, &comp);
        for u in usages.list {
            if declared.contains(&u.names[0].as_str()) {
                continue;
            }
            report_react(
                ctx,
                u.span,
                format!("'{}' is missing in props validation", u.names.join(".")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prop_types() {
        check(
            "S6774",
            "a.jsx",
            r#"
import PropTypes from 'prop-types';
function Hello(props) {
  return <div>{props.name}</div>; // Noncompliant {{'name' is missing in props validation}}
}
function Typed(props) {
  return <div>{props.name}{props.age}</div>; // Noncompliant {{'age' is missing in props validation}}
}
Typed.propTypes = { name: PropTypes.string };
const Destructured = ({ a, b }) => <div>{a}{b}</div>; // Noncompliant
Destructured.propTypes = { a: PropTypes.string };
class Klass extends React.Component {
  static propTypes = { title: PropTypes.string };
  render() {
    return <div>{this.props.title}{this.props.user.name}</div>; // Noncompliant 2
  }
}
function Unresolved(props) { return <div>{props.x}</div>; }
Unresolved.propTypes = sharedPropTypes;
function NotAComponent(props) { return props.x; }
function Other(p) { return <div>{p.x}</div>; }
"#,
        );
    }
}
