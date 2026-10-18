//! In React "this.state" should not be mutated directly
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `getOuterMemberExpression`: the innermost member of a chain whose object has no property.
fn outer_member<'b, 'a>(m: &'b MemberExpression<'a>) -> &'b MemberExpression<'a> {
    let mut cur = m;
    while let Some(inner) = strip_parens(cur.object()).as_member_expression() {
        cur = inner;
    }
    cur
}

fn is_this_state(m: &MemberExpression<'_>) -> bool {
    matches!(m.object(), Expression::ThisExpression(_)) && m.static_property_name() == Some("state")
}

/// Upstream ignores mutations made in the constructor unless they happen while a call
/// expression is being visited (its flag is not a stack: any completed call clears it).
fn ignored_in_constructor(ctx: &Ctx<'_, '_>, id: NodeId, start: u32) -> bool {
    let mut innermost_call = None;
    for a in ancestors(ctx, id) {
        match a.kind() {
            AstKind::CallExpression(_) if innermost_call.is_none() => innermost_call = Some(a),
            AstKind::MethodDefinition(m) => {
                if m.kind != MethodDefinitionKind::Constructor {
                    return false;
                }
                let Some(call) = innermost_call else {
                    return true;
                };
                // A call that completed between the enclosing call's start and the mutation resets the flag.
                let nodes = ctx.nodes();
                let mut i = call.id().index() + 1;
                while i < nodes.len() {
                    let n = nodes.get_node(NodeId::from_usize(i));
                    i += 1;
                    let sp = n.kind().span();
                    if sp.start >= start {
                        break;
                    }
                    if matches!(n.kind(), AstKind::CallExpression(_)) && sp.end <= start {
                        return true;
                    }
                }
                return false;
            }
            AstKind::Class(_) => return false,
            _ => {}
        }
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression, AstType::UpdateExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (report_span, start) = match node.kind() {
            AstKind::AssignmentExpression(a) => {
                let Some(m) = a.left.as_member_expression() else {
                    return;
                };
                if !is_this_state(outer_member(m)) {
                    return;
                }
                (m.object().span(), a.span.start)
            }
            AstKind::UpdateExpression(u) => {
                let Some(m) = u.argument.as_member_expression() else {
                    return;
                };
                let outer = outer_member(m);
                if !is_this_state(outer) {
                    return;
                }
                (outer.span(), u.span.start)
            }
            _ => return,
        };
        let Some(component) = parent_component(ctx, node.id()) else {
            return;
        };
        if !matches!(
            ctx.node(component).kind(),
            AstKind::Class(_) | AstKind::ObjectExpression(_)
        ) {
            return;
        }
        if ignored_in_constructor(ctx, node.id(), start) {
            return;
        }
        ctx.report(report_span, "Do not mutate state directly. Use setState().");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn direct_mutation() {
        check(
            "S6746",
            "a.jsx",
            r#"
import React from 'react';
class MyComponent extends React.Component {
  constructor() {
    super();
    this.state = { count: 0 };
    this.state.count = 1;
    setTimeout(() => { this.state.count = 2; }); // Noncompliant
  }
  incrementCount() {
    this.state.count++; // Noncompliant {{Do not mutate state directly. Use setState().}}
    this.state.nested.value = 1; // Noncompliant
    this.state = {}; // Noncompliant
    this.setState({ count: 1 });
    this.other.count = 1;
  }
  render() { return null; }
}
class NotComponent {
  m() { this.state.x = 1; }
}
"#,
        );
    }
}
