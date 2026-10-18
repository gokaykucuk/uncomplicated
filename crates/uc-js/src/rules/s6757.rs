//! "this" should not be used in functional components
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Sonar: `this` lexically owned by a class member (through arrow functions / the method itself).
fn bound_to_class_member(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut prev = ctx.node(id).kind().span();
    for a in ancestors(ctx, id) {
        let member_value = match a.kind() {
            AstKind::MethodDefinition(m) => Some(m.value.span),
            AstKind::PropertyDefinition(p) => {
                Some(p.value.as_ref().map_or(Span::default(), GetSpan::span))
            }
            AstKind::AccessorProperty(p) => {
                Some(p.value.as_ref().map_or(Span::default(), GetSpan::span))
            }
            _ => None,
        };
        if let Some(v) = member_value {
            return v == prev;
        }
        match a.kind() {
            AstKind::Function(f) if f.is_declaration() => return false,
            AstKind::Function(_)
                if !matches!(ctx.parent_kind(a.id()), AstKind::MethodDefinition(_)) =>
            {
                return false;
            }
            AstKind::StaticBlock(_) => return false,
            _ => {}
        }
        prev = a.kind().span();
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let object = match node.kind() {
            AstKind::StaticMemberExpression(m) => &m.object,
            AstKind::ComputedMemberExpression(m) => &m.object,
            _ => return,
        };
        if !matches!(object, Expression::ThisExpression(_)) {
            return;
        }
        let Some(component) = parent_stateless_component(ctx, node.id()) else {
            return;
        };
        if matches!(
            estree_parent(ctx, component).kind(),
            AstKind::ObjectProperty(_)
        ) {
            return;
        }
        if bound_to_class_member(ctx, node.id()) {
            return;
        }
        ctx.report(
            node.kind().span(),
            "Stateless functional components should not use `this`",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn this_in_sfc() {
        check(
            "S6757",
            "a.jsx",
            r#"
function FunctionalComponent() {
  const value = this.props.value; // Noncompliant {{Stateless functional components should not use `this`}}
  return <div>{value}</div>;
}
class View {
  render() {
    const Row = item => <span>{this.format(item)}</span>;
    return <section>{[1].map(Row)}</section>;
  }
}
class Service {
  make = () => {
    const Row = item => <li>{this.props.format(item)}</li>;
    return Row;
  };
  build() {
    const Row = function () {
      return <li>{this.props.value}</li>; // Noncompliant
    };
    return <Row />;
  }
}
function MyComponent() {
  class Holder {
    [this.props.key]() { return 1; } // Noncompliant
  }
  return <div>{Holder.name}</div>;
}
function notComponent() {
  return this.x;
}
const obj = {
  Foo() { return <div>{this.x}</div>; },
};
"#,
        );
    }
}
